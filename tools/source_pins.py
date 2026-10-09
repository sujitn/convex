"""Match exact source bytes, including Git's explicitly declared CRLF checkout."""
import hashlib
import subprocess


def matches_source_pin(root, path, data, expected):
    if hashlib.sha256(data).hexdigest() == expected:
        return True
    # Only the repository's explicit text/eol=crlf rule admits another physical
    # representation. Binary evidence and LF financial fixtures remain exact.
    result = subprocess.run(['git', 'check-attr', '-z', 'text', 'eol', '--', path],
                            cwd=root, capture_output=True)
    if result.returncode:
        return False
    fields = result.stdout.split(b'\0')
    if len(fields) != 7 or fields[-1] != b'':
        return False
    if fields[0] != path.encode() or fields[3] != path.encode():
        return False
    attributes = {fields[i+1]: fields[i+2] for i in [0, 3]}
    if attributes != {b'text': b'set', b'eol': b'crlf'}:
        return False
    canonical = data.replace(b'\r\n', b'\n')
    if b'\r' in canonical:
        return False
    checkout = canonical.replace(b'\n', b'\r\n')
    if data not in (canonical, checkout):
        return False  # Mixed line endings are not the declared checkout.
    return expected in {hashlib.sha256(canonical).hexdigest(),
                        hashlib.sha256(checkout).hexdigest()}
