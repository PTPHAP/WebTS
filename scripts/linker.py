"""Windows GNU response files need ASCII-relative project paths on Chinese ACP."""
import os
import pathlib
import subprocess
import sys
import tempfile

root = pathlib.Path(__file__).resolve().parent.parent
created = []

def relative(value):
    for prefix in (str(root).replace('\\', '\\\\').replace(' ', '\\ '), str(root).replace('\\', '\\\\'), str(root), root.as_posix()):
        value = value.replace(prefix, '.')
    return value

try:
    arguments = []
    for argument in sys.argv[1:]:
        if argument.startswith('@'):
            path = pathlib.Path(argument[1:])
            data = path.read_bytes()
            encoding = 'utf-16' if data.startswith((b'\xff\xfe', b'\xfe\xff')) else 'utf-8'
            text = relative(data.decode(encoding))
            with tempfile.NamedTemporaryFile(dir=root / '.cache' / 'tmp', suffix='.rsp', delete=False) as file:
                file.write(text.encode('utf-8'))
                created.append(pathlib.Path(file.name))
            arguments.append('@' + os.path.relpath(file.name, root))
        else:
            arguments.append(relative(argument))
    # Cross-target build scripts use the host GCC driver while WASM uses rust-lld.
    options = '\n'.join(arguments) + '\n'.join(path.read_text(encoding='utf-8') for path in created)
    driver = root / '.tools' / ('w64devkit/bin/gcc.exe' if '-Wl,' in options else 'ld.lld.exe')
    if driver.name == 'gcc.exe':
        sysroot = root / '.tools/rustup/toolchains' / os.environ['RUSTUP_TOOLCHAIN'] / 'lib/rustlib/x86_64-pc-windows-gnu/lib/self-contained'
        arguments[:0] = ['-nostartfiles', '-L' + os.path.relpath(sysroot, root), os.path.relpath(sysroot / ('dllcrt2.o' if '-shared' in options else 'crt2.o'), root)]
    result = subprocess.run([str(driver), *arguments], cwd=root)
    sys.exit(result.returncode)
finally:
    for path in created:
        path.unlink(missing_ok=True)
