"""Reports a workspace test run against what cargo built and what the tree expects to fail.

usage: report_tests.py <built log> <test log> <expected failures>

<built log> is the output of `cargo test --no-run`, which names every test binary it built;
<test log> is the output of the test run. A test binary counts only if it ran to a `test result`
line: one that dies loading its libraries, or crashes, never prints one, and cargo's own exit
status hides that among ordinary test failures. Exits 1 if a binary built but did not run to a
result, if a binary hit a loader error, or if a test failed that the expected failures don't
name. Expected failures that passed are reported but don't fail the run.
"""

import re
import sys
from collections.abc import Iterable

EXECUTABLE = re.compile(r"^\s+Executable (.+) \((.+)\)$")
RUNNING = re.compile(r"^\s+Running (.+) \((.+)\)$")
DOC_TESTS = re.compile(r"^\s+Doc-tests (\S+)$")
RESULT = re.compile(r"^test result: ")
FAILED = re.compile(r"^test (.+) \.\.\. FAILED$")
LOADER_ERROR = re.compile(r"error while loading shared libraries|symbol lookup error")


def read_lines(path: str) -> list[str]:
    with open(path, encoding="utf-8", errors="replace") as f:
        return f.read().splitlines()


def built_binaries(lines: Iterable[str]) -> set[str]:
    return {m.group(2) for m in map(EXECUTABLE.match, lines) if m}


def scan_run(lines: Iterable[str]) -> tuple[set[str], list[str], set[str], list[str]]:
    """Returns the binaries that ran to a result, those that started without one, the failed
    test names and the loader error lines."""
    finished: set[str] = set()
    unfinished: list[str] = []
    failed: set[str] = set()
    loader_errors: list[str] = []
    current: str | None = None
    for line in lines:
        running, doc_tests = RUNNING.match(line), DOC_TESTS.match(line)
        if running or doc_tests:
            if current is not None:
                unfinished.append(current)
            if running:
                current = running.group(2)
            elif doc_tests:
                current = "doc-tests " + doc_tests.group(1)
        elif RESULT.match(line):
            if current is not None:
                finished.add(current)
            current = None
        elif failure := FAILED.match(line):
            failed.add(failure.group(1))
        if LOADER_ERROR.search(line):
            loader_errors.append(line.strip())
    if current is not None:
        unfinished.append(current)
    return finished, unfinished, failed, loader_errors


def expected_failures(lines: Iterable[str]) -> set[str]:
    stripped = (line.strip() for line in lines)
    return {line for line in stripped if line and not line.startswith("#")}


def main(argv: list[str]) -> int:
    if len(argv) != 4:
        print(
            "usage: report_tests.py <built log> <test log> <expected failures>",
            file=sys.stderr,
        )
        return 2
    built = built_binaries(read_lines(argv[1]))
    finished, unfinished, failed, loader_errors = scan_run(read_lines(argv[2]))
    expected = expected_failures(read_lines(argv[3]))

    not_run = sorted(built - finished)
    new = sorted(failed - expected)
    passing = sorted(expected - failed)
    doc_tests = sum(1 for name in finished if name.startswith("doc-tests "))

    print(
        f"test binaries: {len(built)} built, {len(built) - len(not_run)} ran to a result"
    )
    for binary in not_run:
        print(f"  did not run to a result: {binary}")
    for binary in unfinished:
        if binary.startswith("doc-tests "):
            print(f"  did not run to a result: {binary}")
    print(f"doc-test runs: {doc_tests} ran to a result")
    print(f"loader errors: {len(loader_errors)}")
    for line in loader_errors:
        print(f"  {line}")
    print(f"failures: {len(failed)}, {len(failed) - len(new)} of them expected")
    for name in new:
        print(f"  new failure: {name}")
    for name in passing:
        print(f"  expected to fail but passed: {name}")

    ok = bool(built) and not not_run and not loader_errors and not new
    ok = ok and not any(b.startswith("doc-tests ") for b in unfinished)
    print("verify tests: ok" if ok else "verify tests: FAILED")
    return 0 if ok else 1


if __name__ == "__main__":
    sys.exit(main(sys.argv))
