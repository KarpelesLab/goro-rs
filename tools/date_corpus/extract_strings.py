import glob, re, json, sys

out = set()
for f in glob.glob("/tmp/claude-1000/php-tests/ext/date/tests/*.phpt") + glob.glob("/tmp/claude-1000/php-tests/ext/date/tests/*.inc"):
    src = open(f, encoding="latin-1").read()
    m = re.search(r"--FILE--\n(.*?)\n--[A-Z]+--", src, re.S)
    body = m.group(1) if m else src
    for q in re.finditer(r"'((?:[^'\\\n]|\\.){1,80})'|\"((?:[^\"\\\n$]|\\.){1,80})\"", body):
        s = q.group(1) if q.group(1) is not None else q.group(2)
        if q.group(1) is not None:
            s = s.replace("\\'", "'").replace("\\\\", "\\")
        else:
            if "\\" in s:
                continue
        if not re.search(r"[0-9]|day|week|month|year|now|noon|midnight|today|tomorrow|yesterday|last|next|first|ago|hour|minute|second|sec|min|mon|tue|wed|thu|fri|sat|sun|jan|feb|mar|apr|may|jun|jul|aug|sep|oct|nov|dec|@|utc|gmt|[A-Z]{3}", s, re.I):
            continue
        out.add(s)
out = sorted(out)
json.dump(out, open(sys.argv[1], "w"))
print(len(out))
