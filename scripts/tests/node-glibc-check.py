"""Check actual ELF symbol requirements rather than inferring them from the build host."""
import argparse
from pathlib import Path
import re
import subprocess

if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', type=Path, required=True)
    args = parser.parse_args()
    output = subprocess.check_output(['readelf', '--version-info', str(args.binary)], text=True)
    versions = {tuple(map(int, v.split('.'))) for v in re.findall(r'GLIBC_(\d+\.\d+(?:\.\d+)?)', output)}
    assert versions and max(versions) <= (2, 35), f'ELF glibc requirement exceeds 2.35: {sorted(versions)}'
    print('ELF glibc <= 2.35: PASS; actual maximum ' + '.'.join(map(str, max(versions))))
