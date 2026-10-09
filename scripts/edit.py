"""edit.py <file> <old_file> <new_file>: replace exact text (line-ending agnostic), fail if absent."""
import sys
p, a, b = sys.argv[1], open(sys.argv[2], encoding='utf-8').read(), open(sys.argv[3], encoding='utf-8').read()
s = open(p, 'rb').read().decode('utf-8')
nl = '\r\n' if '\r\n' in s else '\n'
a, b = a.replace('\r\n', '\n').replace('\n', nl), b.replace('\r\n', '\n').replace('\n', nl)
if a not in s:
    sys.exit(f"not found in {p}")
open(p, 'wb').write(s.replace(a, b, 1).encode('utf-8'))
