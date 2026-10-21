# Animated demo for the README, recorded with https://github.com/charmbracelet/vhs
#   cargo build --release && vhs docs/demo.tape
# Uses --demo, so no accounts, tokens, or real data are involved.
Output docs/demo.gif

Set Shell "bash"
Set FontFamily "JetBrains Mono"
Set FontSize 15
Set Width 900
Set Height 770
Set Padding 18
Set Theme "Catppuccin Mocha"
Set TypingSpeed 55ms
Set Framerate 20
Set PlaybackSpeed 1.0

# Off-camera: use the local release build and pin the clock so every
# render is identical.
Hide
Type `unset NO_COLOR FORCE_COLOR; export PATH="$PWD/target/release:$PATH" PS1='\[\e[32m\]❯\[\e[0m\] ' COLORTERM=truecolor TZ=America/New_York; daypeek() { command daypeek --at 2026-10-21T10:58:00-04:00 "$@"; }; clear`
Enter
Sleep 300ms
Show

Type "daypeek --demo --compact"
Sleep 400ms
Enter
Sleep 2.5s

Ctrl+L
Sleep 300ms
Type "daypeek --demo"
Sleep 400ms
Enter
Sleep 6s
