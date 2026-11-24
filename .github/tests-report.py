import os
import re
import sys
import xml.etree.ElementTree as ET

MOST = 10
SHOWN = 4000
LONGEST = 60000


def escape(text):
    return text.replace("%", "%25").replace("\r", "%0D").replace("\n", "%0A")


def say(kind, title, message):
    title = escape(title).replace(":", "%3A").replace(",", "%2C")
    print(f"::{kind} title={title}::{escape(message)}")


def output(case):
    failure = case.find("failure")
    said = [failure.get("message", "") if failure is not None else ""]
    said += [case.findtext(tag) or "" for tag in ("failure", "system-out", "system-err")]
    return "\n".join(s.strip() for s in said if s.strip())


def excerpt(said, room):
    start = said.find("panicked at")
    return said[start : start + room] if start >= 0 else said[-room:]


def chunks(items, count):
    size = -(-len(items) // count)
    return [items[i : i + size] for i in range(0, len(items), size)]


def summary(lines):
    path = os.environ.get("GITHUB_STEP_SUMMARY")
    if path:
        with open(path, "a", encoding="utf-8") as f:
            f.write("\n".join(lines) + "\n")


def read(log):
    return open(log, encoding="utf-8", errors="replace").read() if os.path.exists(log) else ""


def without_results(system, log):
    text = read(log)
    errors = re.findall(r"^error\[.*?(?:\n\n|\Z)", text, flags=re.S | re.M)[: MOST - 2]
    failed = re.findall(r"^\s+(?:FAIL|TIMEOUT|SIGSEGV|SIGABRT|SIGKILL) \[.*$", text, flags=re.M)
    for e in errors:
        say("error", f"{system} build", e)
    if failed:
        say("error", f"{system} tests that failed", "\n".join(f.strip() for f in failed))
    say("error", f"{system} the end of the test log", text[-SHOWN:])
    summary([f"### {system}: no JUnit file", "", "```", *errors, *failed, "```"])


def main(system, junit, log):
    if not os.path.exists(junit):
        without_results(system, log)
        return
    cases = [
        (suite.get("name", ""), case)
        for suite in ET.parse(junit).getroot().iter("testsuite")
        for case in suite.iter("testcase")
    ]
    failed = [(f"{suite} {case.get('name')}", output(case)) for suite, case in cases if case.find("failure") is not None]
    flaky = [
        f"{suite} {case.get('name')}"
        for suite, case in cases
        if case.find("failure") is None and case.find("flakyFailure") is not None
    ]
    if failed:
        groups = chunks(failed, MOST)
        each = SHOWN // max(len(g) for g in groups) - 120
        for group in groups:
            say(
                "error",
                f"{system} tests failed: " + ", ".join(name.split("::")[-1] for name, _ in group),
                "\n\n".join(f"#### {name}\n{excerpt(said, each)}" for name, said in group),
            )
    if flaky:
        say("warning", f"{system} flaky tests, passed on a retry", "\n".join(flaky))
    summary(
        [f"### {system}: {len(cases)} tests, {len(failed)} failed, {len(flaky)} flaky", ""]
        + [f"<details><summary>{name}</summary>\n\n```\n{said[-LONGEST:]}\n```\n</details>" for name, said in failed]
        + [f"- flaky: {name}" for name in flaky]
    )


main(*sys.argv[1:4])
