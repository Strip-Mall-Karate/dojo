"""Tests report_tests.py on logs shaped like cargo's.

Run: python3 -I -m unittest discover -s support/vm/guest
"""

import contextlib
import io
import os
import tempfile
import unittest

import report_tests

BUILT = """\
   Compiling dojo_core v0.0.0
    Finished `test` profile [unoptimized + debuginfo] target(s) in 1.00s
  Executable unittests src/lib.rs (target/debug/deps/dojo_core-1111)
  Executable tests/it.rs (target/debug/deps/it-2222)
"""

CLEAN_RUN = """\
     Running unittests src/lib.rs (target/debug/deps/dojo_core-1111)

running 2 tests
test a::works ... ok
test a::known ... FAILED

test result: FAILED. 1 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s

     Running tests/it.rs (target/debug/deps/it-2222)

running 1 test
test it_works ... ok

test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s

   Doc-tests dojo_core

running 0 tests

test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
"""

LOADER_ERROR = """\
     Running unittests src/lib.rs (target/debug/deps/dojo_core-1111)

running 1 test
test a::known ... FAILED

test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s

     Running tests/it.rs (target/debug/deps/it-2222)
target/debug/deps/it-2222: symbol lookup error: libdl.so.2: undefined symbol: _dl_catch_error
error: test failed, to rerun pass `--test it`
"""

CRASHED = CLEAN_RUN.replace(
    """test it_works ... ok

test result: ok. 1 passed""",
    """error: test failed, to rerun pass `--test it`

Caused by:
  process didn't exit successfully: (signal: 11, SIGSEGV: invalid memory reference)
X""",
)


class ReportTests(unittest.TestCase):
    def report(self, run: str, expected: str = "a::known\n") -> tuple[int, str]:
        with tempfile.TemporaryDirectory() as tmp:
            paths = []
            for name, text in (("built", BUILT), ("run", run), ("expected", expected)):
                path = os.path.join(tmp, name)
                with open(path, "w", encoding="utf-8") as f:
                    f.write(text)
                paths.append(path)
            out = io.StringIO()
            with contextlib.redirect_stdout(out):
                status = report_tests.main(["report_tests.py", *paths])
        return status, out.getvalue()

    def test_a_run_matching_the_expected_failures_is_ok(self) -> None:
        status, out = self.report(CLEAN_RUN)
        self.assertEqual(status, 0, out)
        self.assertIn("test binaries: 2 built, 2 ran to a result", out)

    def test_a_binary_that_never_reports_a_result_fails_the_run(self) -> None:
        status, out = self.report(LOADER_ERROR)
        self.assertEqual(status, 1, out)
        self.assertIn("did not run to a result: target/debug/deps/it-2222", out)
        self.assertIn("loader errors: 1", out)

    def test_a_binary_that_crashes_fails_the_run(self) -> None:
        status, out = self.report(CRASHED)
        self.assertEqual(status, 1, out)
        self.assertIn("did not run to a result: target/debug/deps/it-2222", out)
        self.assertIn("loader errors: 0", out)

    def test_a_failure_not_expected_fails_the_run(self) -> None:
        status, out = self.report(CLEAN_RUN, expected="# none expected\n")
        self.assertEqual(status, 1, out)
        self.assertIn("new failure: a::known", out)

    def test_an_expected_failure_that_passes_is_reported_but_ok(self) -> None:
        status, out = self.report(CLEAN_RUN, expected="a::known\nb::fixed\n")
        self.assertEqual(status, 0, out)
        self.assertIn("expected to fail but passed: b::fixed", out)

    def test_nothing_built_fails_the_run(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            empty = os.path.join(tmp, "empty")
            with open(empty, "w", encoding="utf-8"):
                pass
            with contextlib.redirect_stdout(io.StringIO()):
                self.assertEqual(
                    report_tests.main(["report_tests.py", empty, empty, empty]), 1
                )


if __name__ == "__main__":
    unittest.main()
