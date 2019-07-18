# WSL Script

Shell script _(.sh)_ handler for
[Windows Subsystem for Linux](https://docs.microsoft.com/en-us/windows/wsl/about) _(WSL)_.

Registers .sh extension to be executed in WSL.
Automatically handles Windows → Unix path conversions.
Files can be dragged and dropped to .sh file icon in explorer to pass
other files as arguments.

## Usage

Move `wslscript.exe` file to a location of your choice.
Don't move the file afterwards.

Run `wslscript.exe` to open a GUI.
Click a button to add .sh extension to Windows registry.

After registration, `.sh` files can be executed from explorer by double clicking.
Other files can be passed as path arguments by dragging and dropping them into
`.sh` file icon.

Note that Drag & Drop handler may not work until reboot.

## TODO

- Multiple configurable file extensions
- WSL distro selection
- Exit modes (leave terminal open on error, always close, etc.)
- Optionally register for all users

## License

This project is licensed under the
[MIT License](https://github.com/sop/wslscript/blob/master/LICENSE).

Icon by [Tango Desktop Project](http://tango.freedesktop.org/Tango_Desktop_Project).
