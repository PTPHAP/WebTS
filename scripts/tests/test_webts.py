import contextlib
import importlib.util
import io
import json
import os
from pathlib import Path
import subprocess
import tempfile
import tomllib
import unittest
from unittest.mock import patch

spec=importlib.util.spec_from_file_location('webts_manager',Path(__file__).parents[1]/'webts.py')
manager=importlib.util.module_from_spec(spec);spec.loader.exec_module(manager)


class ManagementTests(unittest.TestCase):
    def setUp(self):
        temp=Path(__file__).resolve().parents[2]/'.cache/tmp';temp.mkdir(parents=True,exist_ok=True)
        self.folder=tempfile.TemporaryDirectory(dir=temp)
        self.root=Path(self.folder.name)
        self.root_patch=patch.object(manager,'ROOT',self.root);self.root_patch.start()
        self.config={'domain':'voice.example.com','ip':'8.8.8.8','port':18080,'server':'ts.example.com:9987','admin_email':'owner@example.com','smtp_host':'smtp.example.com','smtp_port':465,'smtp_user':'mail@example.com','smtp_from':'mail@example.com','proxy':'external'}
        for name in ['data','secrets','releases']: (self.root/name).mkdir()
        (self.root/'secrets/master.key').write_text('test key must stay unchanged')
        (self.root/'secrets/smtp-password').write_text('private test SMTP value')
        # Windows tests verify generated files; Linux CI verifies real owner permissions.
        self.chown=patch.object(manager.os,'chown',create=True);self.chown.start()
        manager.save(self.config)

    def tearDown(self):
        self.chown.stop();self.root_patch.stop();self.folder.cleanup()

    def test_generated_config_and_compose_are_safe_and_keep_secrets_separate(self):
        config=tomllib.loads((self.root/'config.local.toml').read_text(encoding='utf-8'))
        compose=json.loads((self.root/'compose.json').read_text())
        self.assertEqual(config['smtp']['password_file'],'secrets/smtp-password')
        self.assertFalse(config['allow_insecure_localhost'])
        self.assertEqual(config['bind'],'127.0.0.1:18080')
        self.assertEqual(set(compose['services']),{'webts'})
        self.assertTrue(compose['services']['webts']['read_only'])
        self.assertEqual(compose['services']['webts']['cap_drop'],['ALL'])
        self.assertNotIn('private test SMTP value',(self.root/'setup.json').read_text())
        for value in ['localhost:9987','127.0.0.1:9987','169.254.169.254:80','ts.example.com;rm -rf /','https://ts.example.com','user@ts.example.com']:
            self.assertFalse(manager.target(value),value)
        self.assertTrue(manager.target('ts.example.com:9987'))
        self.assertFalse(manager.domain('example.com\n:443 { evil }'))
        if os.name=='posix':self.assertEqual((self.root/'config.local.toml').stat().st_mode&0o777,0o600)

    def test_private_files_refuse_symlink_overwrite(self):
        if os.name!='posix':self.skipTest('Linux link permissions')
        other=self.root/'outside';other.write_text('preserve')
        link=self.root/'bad';link.symlink_to(other)
        with self.assertRaises(ValueError):manager.private_write(link,'secret')
        self.assertEqual(other.read_text(),'preserve')

    def test_operator_credentials_never_enter_argv_container_logs_or_network(self):
        with patch.object(manager,'run') as run:
            manager.tool('set-settings',text='operator-private-value',capture=True)
        args=run.call_args.args[0]
        self.assertEqual(args[args.index('--log-driver')+1],'none')
        self.assertEqual(args[args.index('--network')+1],'none')
        self.assertNotIn('operator-private-value',args)
        self.assertEqual(run.call_args.kwargs['text'],'operator-private-value')

    def test_site_configuration_preserves_credentials_identities_and_custom_policy(self):
        previous={'home':{'site_name':'Old','privacy_policy':'Custom policy','terms':'Custom terms','site_icon':'saved-icon'},'smtp':{'password':'private-value'},'servers':[{'id':'saved'}]}
        public={'site_name':'新站点','operator':'公开运营者','contact':'privacy@example.com','data_details':'独立测试环境'}
        calls=[]
        with patch.object(manager,'settings',return_value=previous),patch.object(manager,'site_values',return_value=public),patch.object(manager,'compose'),patch.object(manager,'tool',side_effect=lambda c,**kw:calls.append((c,kw))),patch.object(manager,'health'),contextlib.redirect_stdout(io.StringIO()) as output:
            manager.configure('site')
        updated=json.loads(calls[0][1]['text'])
        self.assertEqual(updated['home']['site_name'],'新站点')
        for field in ['privacy_policy','terms','site_icon']:self.assertEqual(updated['home'][field],previous['home'][field])
        self.assertEqual(updated['smtp'],previous['smtp']);self.assertEqual(updated['servers'],previous['servers'])
        self.assertNotIn('private-value',output.getvalue())
        self.assertEqual((self.root/'secrets/master.key').read_text(),'test key must stay unchanged')

    def test_mail_change_preserves_other_settings_and_never_puts_password_in_argv(self):
        previous={'servers':[{'id':'custom','name':'Existing','address':'ts.example.com:9988'}],'default_server':'custom','allow_custom':True,'smtp':{'host':'smtp.example.com','port':465,'username':'mail@example.com','from':'mail@example.com','password':'old-private'}}
        next_mail={**previous['smtp'],'password':'new-private'}
        commands=[]
        def tool(command,**kwargs):commands.append((command,kwargs))
        with patch.object(manager,'settings',return_value=previous),patch.object(manager,'smtp_values',return_value=next_mail),patch.object(manager,'compose'),patch.object(manager,'tool',side_effect=tool),patch.object(manager,'health'),contextlib.redirect_stdout(io.StringIO()) as output:
            manager.configure('smtp')
        self.assertEqual(commands[0][0],'set-settings')
        written=json.loads(commands[0][1]['text'])
        self.assertEqual(written['servers'],previous['servers'])
        self.assertEqual(written['default_server'],'custom')
        self.assertTrue(written['allow_custom'])
        self.assertNotIn('new-private',output.getvalue())
        self.assertNotIn('new-private',(self.root/'setup.json').read_text())
        self.assertEqual((self.root/'secrets/master.key').read_text(),'test key must stay unchanged')

    def test_config_failure_rolls_back_settings_without_replacing_user_database(self):
        original_config=(self.root/'config.local.toml').read_bytes()
        previous={'servers':[],'default_server':'','allow_custom':False,'smtp':{'host':'smtp.example.com','port':465,'username':'mail@example.com','from':'mail@example.com','password':'old-private'}}
        commands=[]
        with patch.object(manager,'settings',return_value=previous),patch.object(manager,'smtp_values',return_value={**previous['smtp'],'host':'new.example.com','password':'new-private'}),patch.object(manager,'compose'),patch.object(manager,'tool',side_effect=lambda c,**kw:commands.append((c,kw))),patch.object(manager,'health',side_effect=ValueError('failed')),contextlib.redirect_stdout(io.StringIO()):
            with self.assertRaisesRegex(ValueError,'恢复原配置'):manager.configure('smtp')
        self.assertEqual((self.root/'config.local.toml').read_bytes(),original_config)
        self.assertEqual((self.root/'secrets/smtp-password').read_text(),'private test SMTP value')
        self.assertEqual(json.loads(commands[-1][1]['text']),previous)
        self.assertEqual((self.root/'secrets/master.key').read_text(),'test key must stay unchanged')

    def test_backup_uses_sqlite_consistent_backup_and_keeps_original_key(self):
        import sqlite3
        with contextlib.closing(sqlite3.connect(self.root/'data/web-ts.db')) as db:
            db.execute('CREATE TABLE identities(uid TEXT)');db.execute("INSERT INTO identities VALUES('keep-uid')")
            db.commit()
        with contextlib.redirect_stdout(io.StringIO()):folder=manager.backup()
        with contextlib.closing(sqlite3.connect(folder/'web-ts.db')) as db:self.assertEqual(db.execute('SELECT uid FROM identities').fetchone()[0],'keep-uid')
        self.assertEqual((folder/'secrets/master.key').read_bytes(),(self.root/'secrets/master.key').read_bytes())


if __name__=='__main__':unittest.main()
