import os

import pexpect

COLUMNS = 64
LINES = 16


def is_evil_helix() -> bool:
	return not is_vim()


def is_vim() -> bool:
	return os.environ.get("TEST_TARGET") == "vim"


class App(pexpect.spawn):
	def __init__(self):
		"""
		Spawn the test target using `COLUMNS` x `LINES` dimensions.

		By default, `hx` is started.
		If the environment variable `TEST_TARGET` is set to `vim`, then `vim` is started instead.
		"""
		bin = "hx"
		# args = ["-c", "/dev/null"]
		args = []
		name = "evil-helix"

		if is_vim():
			bin = "vim"
			args = ["-u", "/dev/null"]
			name = "Vim"

		print(f"Start {name} ({bin} {args})")
		super().__init__(bin, args=args, timeout=3)  # , dimensions=(LINES, COLUMNS))

	def is_evil_helix(self) -> bool:
		return is_evil_helix()

	def is_vim(self) -> bool:
		return is_vim()

	def send_escape(self):
		print("Press escape")
		super().send("\x1b")

	def quit(self):
		print("Quit")
		super().sendline(":quit")
