$version = Get-Content .\wslscript\Cargo.toml |
    Select-String -Pattern '^version = "([^"]+)"' |
    Select-Object -First 1 | ForEach-Object {
        $_.Matches.Groups[1].Value
    }
$buildir = "build"
New-Item -ItemType Directory -Name $buildir -ErrorAction Ignore
$srcdir = "target\release"
$a = @{
    DestinationPath = "$buildir\wslscript-$version.zip"
    Path            = "$srcdir\wslscript.exe", "$srcdir\wslscript_handler.dll" 
    Force           = $true
}
Compress-Archive @a
