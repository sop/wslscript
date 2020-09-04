# [WSL Script](https://sop.github.io/wslscript/)

Shell script _(.sh)_ handler for
[Windows Subsystem for Linux](https://docs.microsoft.com/en-us/windows/wsl/about) _(WSL)_.

Associates .sh _(or any other)_ extension to be executed in WSL.
Automatically handles Windows → Unix path conversions.
Files can be dragged and dropped to registered file icon in explorer
to pass paths as arguments.

## Usage

Copy `wslscript.exe` file to a location of your choice.
This executable is used to invoke WSL, so don't move the file afterwards.

Run `wslscript.exe` to open a setup GUI.
Enter the extension and click _Register_ button to add filetype association
into Windows registry.

After registration, `.sh` files can be executed from explorer by double clicking.
Other files can be passed as path arguments by dragging and dropping them into
`.sh` file icon.

Scripts are executed in the same folder where the script file is located,
ie. `$PWD` is set to script's directory.

Note that Drag & Drop handler may not work until reboot.

## TODO

- [ ] Optionally register for all users

## License

This project is licensed under the
[MIT License](https://github.com/sop/wslscript/blob/master/LICENSE).

Icon by [Tango Desktop Project](http://tango.freedesktop.org/Tango_Desktop_Project).
