# `daypeek --watch` for the README, recorded with https://github.com/charmbracelet/vhs
#   cargo build --release && vhs docs/watch.tape
# Uses --demo, so no accounts, tokens, or real data are involved. The clock
# starts at 10:59:52 so the "now" marker visibly moves at 11:00.
Output docs/watch.gif

Set Shell "bash"
Set FontFamily "JetBrains Mono"
Set FontSize 15
Set Width 900
Set Height 760
Set Padding 18
Set Theme "Catppuccin Mocha"
Set TypingSpeed 55ms
Set Framerate 20
Set PlaybackSpeed 1.0

Hide
Type `unset NO_COLOR FORCE_COLOR; export PATH="$PWD/target/release:$PATH" PS1='\[\e[32m\]❯\[\e[0m\] ' COLORTERM=truecolor TZ=America/New_York; daypeek() { command daypeek --at 2026-10-21T10:59:50-04:00 "$@"; }; clear`
Enter
Sleep 300ms
Show

Type "daypeek --demo --watch --every 2"
Sleep 400ms
Enter
Sleep 12s
Type "q"
Sleep 1.5s
