#!/usr/bin/env -S uv run

import common

app = common.App()

if app.is_evil_helix():
	print("Expect an initial line number")
	app.expect("    1")

print("Press v")
app.send("v")

mode = "VIS" if not app.is_vim() else "VISUAL"
print(f'Expect " {mode} " in the mode indicator')
app.expect(f" {mode} ")

app.send_escape()

if app.is_evil_helix():
	mode = "NOR"
	print(f'Expect " {mode} " in the mode indicator')
	app.expect(f" {mode} ")

app.quit()
