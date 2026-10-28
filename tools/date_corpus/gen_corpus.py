import json, random, sys

random.seed(12345)
C = []
add = C.append

# time formats
for s in ["4 pm", "4pm", "4 A.M.", "4a.m.", "12am", "12pm", "0am", "13pm", "4:08 am", "7:19P.M.", "4:08:37 am", "7:19:19P.M.",
          "4:08:37.81412 am", "4:08:37:81412 am", "04:08", "19.19", "T23:43", "t2343", "0408", "t1919", "T2343",
          "04.08.37", "19:19:19", "T23:43:01", "040837", "t191919", "04:08:37 CEST", "04:08:37+0200", "04:08:37 +02:00",
          "04:08:37.81412", "19:19:19.532453", "24:00", "24:00:00", "24:01", "25:00", "23:60", "23:59:60", "23:59:61",
          "1:2", "1:2:3", "01:02:03.1234567", "12:00:00.0", "12.5", "12:00 GMT+2", "12:00 (CEST)", "12:00 CEST",
          "12:00 Europe/Amsterdam", "12:00 America/Argentina/Buenos_Aires", "12:00 Foo/Bar", "12:00 Z", "12:00z", "12:00 UTC",
          "12:00 utc", "12:00 EDT", "12:00 PST", "12:00 XYZ", "12:00 ABCDEFG", "12:00 +5", "12:00 -05", "12:00 +0530",
          "12:00 +05:30", "12:00 +05:30:15", "12:00 -123", "12:00 +12345", "12:00 +123456", "12:00 GMT-05:00",
          "12:00 GMT", "12:00 +99:99", "12:00 +100", "12:00 (UTC)", "12:00 ( UTC )", "noon", "midnight", "12 noon",
          "1.5", "10.30", "10.30pm", "10:30 p.m.", "10:30 pm.", "10:30pm GMT", "11 oclock", "8h", "8h30", "T8", "t8:00",
          "12:00:00 am", "00:00:00 pm", "12:00 PM", "11:59:59 PM", "1 am", "1a", "1p", "1 p", "13:00 pm"]:
    add(s)
# date formats
for s in ["5/12", "12/22/78", "1/17/2006", "1/17/06", "13/17/2006", "2/30/2008", "2008/6/30", "1978/12/22", "2008-6",
          "2008-06", "2008-6-30", "1978-12-22", "2008-06-31", "2008-13-01", "2008-00-00", "0000-00-00", "30-6-2008",
          "22.12.1978", "30.6.08", "22\t12.78", "30-June 2008", "22DEC78", "14 III 1879", "June 2008", "DEC1978",
          "March 1879", "2008 June", "1978-XII", "July 1st, 2008", "April 17, 1790", "May.9,78", "July 1st", "Apr 17",
          "May.9", "1 July", "17 Apr", "9.May", "May-09-78", "Apr-17-1790", "78-Dec-22", "1814-MAY-17", "1978", "2008",
          "-81120", "+260663", "March", "jun", "DEC", "Sept", "sept 5", "September", "15810726", "19780417", "18140517",
          "2008/06/30", "08-06-30", "78-12-22", "1978-12-22T", "2008-W27", "2008W27", "2008W273", "2008-W28-3",
          "2008-W53", "2008-W54", "2008-W00", "2008-W28-8", "2008.197", "2008197", "2008.366", "2008.000",
          "2008:08:07 18:11:31", "2008-08-07 18:11:31", "2008-07-01T22:35:17.02", "2008-07-01T22:35:17.03+08:00",
          "2008-07-01T22:35:17Z", "2008-07-01T22:35:17.123456789Z", "@1215282385", "@-1215282385", "@1215282385.5",
          "@1215282385.123456", "@1215282385.1234567", "@ 1215282385", "@1215282385 UTC", "@1215282385 +02:00", "@",
          "@-", "@1.", "@.5", "20080701T22:38:07", "20080701t223807", "20080701T223807", "2008-7-1T9:3:37",
          "2008-7-1T9:03:37", "Mon, 15 Aug 2005 15:52:01 +0000", "Monday, 15-Aug-05 15:52:01 UTC",
          "Mon, 15 Aug 05 15:52:01 +0000", "2005-08-15T15:52:01+00:00", "2005-08-15T15:52:01+0000",
          "10/Oct/2000:13:55:36 -0700", "10/Oct/2000:13:55:36 +00:00", "Tue Jul 1 22:35:17 2008", "Tue Jul  1 22:35:17 CEST 2008",
          "Sat, 01 Jan 2000 00:00:00 GMT", "2000-01-01 00:00:00 GMT+2", "1 January 2000", "January 1 2000",
          "January 1, 2000 10:00", "Jan 1 2000 10am", "2000 Jan 1", "1-Jan-2000", "01-JAN-00", "2000-Jan-01",
          "Jan-01-2000", "2000.01.01", "01.01.2000", "1.1.2000", "01.01.00", "01/01/2000", "2000/01/01", "2000-1-1",
          "0000-01-01", "0001-01-01", "9999-12-31", "10000-01-01", "99999-01-01", "-0001-01-01", "-2000-01-01",
          "+2000-01-01", "+10000-01-01", "000000-01-01", "12345", "123456", "1234567", "123456789", "12345678901",
          "2000-02-30", "2000-02-31", "2000-04-31", "2001-02-29", "2000-02-29", "31.04.2008", "31 April 2008",
          "February 30, 2008", "Feb 31", "0", "00", "000", "0000", "1", "12", "123", "99", "100", "2020-1", "1-2020"]:
    add(s)
# relative formats
for s in ["now", "today", "tomorrow", "yesterday", "midnight", "noon", "back of 7pm", "front of 7", "back of 19",
          "front of 0", "back of 23", "front of 12am", "first day of", "last day of", "first day of next month",
          "last day of next month", "first day of this month", "last day of last month", "first day of january",
          "first day of january 2008", "last day of february 2008", "first day", "last day", "first monday of",
          "first monday of next month", "last friday of next month", "second tuesday of january 2009",
          "third wednesday of", "last sunday of march", "next monday of", "this monday of", "+1 day", "-1 day", "+1 days",
          "1 day", "1day", "+1day", "+ 1 day", "- 1 day", "++1 day", "--1 day", "+-1 day", "-+1 day", "+1 week",
          "+2 weeks", "1 fortnight", "1 forthnight", "+1 month", "+1 year", "+1 hour", "+1 hours", "+1 min", "+1 mins",
          "+1 minute", "+1 minutes", "+1 sec", "+1 secs", "+1 second", "+1 seconds", "+1 msec", "+1 millisecond",
          "+1 milliseconds", "+1 ms", "+1 usec", "+1 microsecond", "+1 microseconds", "+1 µs", "+1 µsec",
          "+1 weekday", "+3 weekdays", "-3 weekdays", "+1 week 2 days 4 hours 2 seconds", "next thursday",
          "last monday", "-1 week", "yesterday noon", "noon tomorrow", "tomorrow noon", "6 month ago 2 days",
          "2 days ago", "ago", "1 day ago ago", "+1 day ago", "3 years ago", "next week", "last week", "this week",
          "previous week", "next month", "last month", "this month", "next year", "last year", "this year",
          "next day", "last day", "next hour", "last hour", "next minute", "next sec", "next fortnight",
          "first week", "second week", "third month", "fourth year", "fifth day", "sixth hour", "seventh minute",
          "eighth second", "ninth week", "tenth day", "eleventh day", "twelfth day", "thirteenth day", "next weekday",
          "last weekday", "this weekday", "monday", "Monday", "mon", "MON", "tuesday", "tue", "tues", "wed",
          "wednesday", "thu", "thur", "thurs", "thursday", "fri", "friday", "sat", "saturday", "sun", "sunday",
          "monday next week", "next monday", "last monday", "this monday", "previous monday", "monday this week",
          "sunday last week", "first monday", "second monday", "last sunday", "monday 10:00", "10:00 monday",
          "monday +1 week", "+1 week monday", "mon, 1 jan 2001", "monday 2001-01-01", "2001-01-01 monday",
          "tomorrow 11:00", "11:00 tomorrow", "midnight tomorrow", "today 10:00", "10:00 today", "now + 1 day",
          "now 10:00", "+1 day 10:00", "10:00 +1 day", "2008-01-01 +1 day", "+1 day 2008-01-01", "2008-01-01 next month",
          "first day of next month midnight", "midnight first day of next month", "last day of feb", "last day of +1 month",
          "+1 month last day of", "first day of -1 month", "+1 month first day of", "1 week ago", "-1 week ago",
          "+1 week ago", "2 weeks", "two weeks", "a week", "one day", "next", "last", "this", "first", "+", "-",
          "+1", "-1", "1 sat", "+1 sat", "-1 sat", "+2 sat", "2 monday", "-2 friday", "next sat", "last sat",
          "this sat", "sat next week", "sat this week", "sat last week", "weekday", "weekdays", "1 weekday ago",
          "+1 week sat", "saturday this week", "sunday this week", "monday last week", "this week monday",
          "this week sunday", "next week sunday", "last week saturday", "+1 year -1 day", "-1 year +1 day",
          "+12 months", "+1000 years", "+2147483648 days", "+9223372036854775807 seconds", "+9223372036854775808 seconds",
          "-9223372036854775808 seconds", "+99999999999999999999 days", "1 day 1 day", "+1 day +1 day", "+1 hour 30 minutes",
          "+1.5 hours", "+1,5 hours", "1 hour ago 30 minutes", "next monday 9am", "last day of this month 23:59:59",
          "first sat of July 2008", "last wed of next month", "first day of 2008-02", "2008-02 last day of",
          "noon yesterday", "yesterday 14:00", "back of noon", "front of midnight", "tomorrow midnight", "midnight today",
          "today midnight", "today noon", "now noon", "noon now", "+1 days ago", "eighth day of", "twelfth monday of"]:
    add(s)
# combinations and garbage
words = ["2008-01-01", "10:00", "+1 day", "next monday", "UTC", "CEST", "Europe/Paris", "+0200", "noon", "first day of",
         "tomorrow", "@123", "2008", "Jan", "1", "x", "foo", "  ", "\t", ",", ".", "/", "-", "+", ":", "(", ")", "T", "t",
         "am", "pm", "ago", "of", "W", "Z"]
for _ in range(700):
    n = random.randint(1, 4)
    add(" ".join(random.choice(words) for _ in range(n)))
for _ in range(400):
    n = random.randint(1, 8)
    add("".join(random.choice("0123456789:-/. TtWZ+@aAmMpP") for _ in range(n)))
# leading/trailing whitespace
for s in [" 2008-01-01", "2008-01-01 ", "\t10:00\t", "\n2008-01-01\n", "2008-01-01\0", "\x00", "", " ", "  now  ",
          "2008-01-01\v", "2008-01-01\f", "\x0b10:00", "10:00\r\n"]:
    add(s)

seen = set()
out = []
for s in C:
    if s not in seen:
        seen.add(s)
        out.append(s)
tests = json.load(open(sys.argv[1]))
for s in tests:
    if s not in seen:
        seen.add(s)
        out.append(s)
json.dump(out, open(sys.argv[2], "w"))
print(len(out))
