import json, random, sys

random.seed(777)
months = ["Jan", "jan", "January", "FEB", "february", "Mar", "march", "Apr", "May", "June", "jul", "Aug", "Sep", "Sept", "sept.",
          "October", "Nov", "Dec", "XII", "IV", "ix"]
days = ["Mon", "monday", "Tue", "TUESDAY", "wed", "Thursday", "fri", "Sat", "sunday", "weekday", "weekdays"]
units = ["sec", "secs", "second", "seconds", "min", "mins", "minute", "minutes", "hour", "hours", "day", "days", "week",
         "weeks", "fortnight", "month", "months", "year", "years", "msec", "ms", "usec", "µs", "weekday", "weekdays"]
reltext = ["next", "last", "previous", "this", "first", "second", "third", "twelfth", "eight"]
zones = ["UTC", "GMT", "Z", "EST", "edt", "CEST", "PST", "MSK", "Europe/Paris", "America/New_York", "Asia/Tokyo",
         "+0100", "-05:00", "+5", "-0330", "+05:30:00", "GMT+2", "GMT-05:30", "(CET)", "XYZ", "A", "Q", "foo/Bar"]


def num(a, b, pad=0):
    v = random.randint(a, b)
    return str(v).zfill(pad) if pad else str(v)


def tok():
    r = random.random()
    k = random.randint(0, 40)
    if k == 0:
        return "%s-%s-%s" % (num(1900, 2100), num(1, 12, 2), num(1, 31, 2))
    if k == 1:
        return "%s/%s/%s" % (num(1, 12), num(1, 31), num(0, 2100))
    if k == 2:
        return "%s:%s" % (num(0, 24), num(0, 59, 2))
    if k == 3:
        return "%s:%s:%s" % (num(0, 24), num(0, 59, 2), num(0, 60, 2))
    if k == 4:
        return "%s:%s:%s.%s" % (num(0, 23, 2), num(0, 59, 2), num(0, 59, 2), num(0, 999999999))
    if k == 5:
        return "%s %s" % (num(1, 12), random.choice(["am", "pm", "a.m.", "P.M.", "AM"]))
    if k == 6:
        return "%s:%s%s" % (num(1, 12), num(0, 59, 2), random.choice(["am", "pm", " am", " PM", "p.m."]))
    if k == 7:
        return random.choice(months)
    if k == 8:
        return "%s %s" % (random.choice(months), num(1, 31))
    if k == 9:
        return "%s %s, %s" % (random.choice(months), num(1, 31), num(0, 2100))
    if k == 10:
        return "%s %s %s" % (num(1, 31), random.choice(months), num(0, 2100))
    if k == 11:
        return random.choice(days)
    if k == 12:
        return "%s%s %s" % (random.choice(["+", "-", "", "+ ", "- "]), num(0, 100), random.choice(units))
    if k == 13:
        return "%s %s" % (random.choice(reltext), random.choice(units + days))
    if k == 14:
        return random.choice(zones)
    if k == 15:
        return random.choice(["now", "today", "tomorrow", "yesterday", "midnight", "noon", "ago"])
    if k == 16:
        return "%s day of" % random.choice(["first", "last"])
    if k == 17:
        return "%s %s of" % (random.choice(reltext), random.choice(days[:-2]))
    if k == 18:
        return "@%s" % num(-2000000000, 2000000000)
    if k == 19:
        return "%s week" % random.choice(["next", "last", "this", "previous"])
    if k == 20:
        return "%sW%s" % (num(1990, 2030), num(0, 54, 2))
    if k == 21:
        return "%s-W%s-%s" % (num(1990, 2030), num(1, 53, 2), num(0, 8))
    if k == 22:
        return "%s.%s" % (num(1990, 2030), num(1, 366, 3))
    if k == 23:
        return "%s%s%s" % (num(1990, 2030), num(1, 12, 2), num(1, 31, 2))
    if k == 24:
        return "%s%s%sT%s:%s:%s" % (num(1990, 2030), num(1, 12, 2), num(1, 31, 2), num(0, 23), num(0, 59, 2), num(0, 59, 2))
    if k == 25:
        return "%s.%s.%s" % (num(1, 31), num(1, 12), num(0, 2100))
    if k == 26:
        return "%s-%s-%s" % (num(1, 31), random.choice(months[:6]), num(0, 2100))
    if k == 27:
        return "%s %s" % (random.choice(["back of", "front of"]), num(0, 24))
    if k == 28:
        return "%s:%s:%s %s:%s:%s" % (num(1990, 2030), num(1, 12, 2), num(1, 31, 2), num(0, 23, 2), num(0, 59, 2), num(0, 59, 2))
    if k == 29:
        return "%s/%s/%s:%s:%s:%s %s" % (num(1, 31, 2), random.choice(months[:6]), num(1990, 2030), num(0, 23, 2), num(0, 59, 2), num(0, 59, 2), random.choice(["+0200", "-0700"]))
    if k == 30:
        return num(0, 99999999)
    if k == 31:
        return "%s-%s" % (num(1990, 2030), num(1, 12))
    if k == 32:
        return "t%s%s" % (num(0, 23, 2), num(0, 59, 2))
    if k == 33:
        return "%s %s" % (num(1990, 2030), random.choice(months))
    if k == 34:
        return "%s, %s %s %s %s:%s:%s %s" % (random.choice(days[:-2]), num(1, 31, 2), random.choice(months[:12]), num(1990, 2030), num(0, 23, 2), num(0, 59, 2), num(0, 59, 2), random.choice(zones))
    if k == 35:
        return "%s-%s-%sT%s:%s:%s%s" % (num(1990, 2030), num(1, 12, 2), num(1, 31, 2), num(0, 23, 2), num(0, 59, 2), num(0, 59, 2), random.choice(["Z", "+02:00", ".5Z", ".123-0300", ""]))
    if k == 36:
        return random.choice(["+", "-", "/", ":", "(", ")", ",", ".", "'", "\"", "x", "T", "t", "W", "of", "st", "nd", "th"])
    if k == 37:
        return "%s%s" % (random.choice(["+", "-"]), num(0, 99999, 0))
    if k == 38:
        return "%s %s %s" % (random.choice(reltext), random.choice(days[:-2]), random.choice(["next week", "this week", "last week", "of next month", "of"]))
    if k == 39:
        return "%s-%s-%s" % (num(0, 99, 2), random.choice(months), num(1, 31, 2))
    return "%s%s" % (random.choice(months), num(1, 2100))


out = set()
while len(out) < 6000:
    n = random.randint(1, 4)
    sep = random.choice([" ", " ", " ", "", ", ", "  ", "\t", "T"])
    out.add(sep.join(tok() for _ in range(n)))
json.dump(sorted(out), open(sys.argv[1], "w"))
print(len(out))
