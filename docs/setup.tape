# The `daypeek setup` wizard, in demo mode (fake input, nothing is saved).
#   cargo build --release && vhs docs/setup.tape
Output docs/setup.gif

Set Shell "bash"
Set FontFamily "JetBrains Mono"
Set FontSize 14
Set Width 900
Set Height 1300
Set Padding 18
Set Theme "Catppuccin Mocha"
Set TypingSpeed 40ms
Set Framerate 15

Hide
Type `unset NO_COLOR FORCE_COLOR; export PATH="$PWD/target/release:$PATH" PS1='\[\e[32m\]❯\[\e[0m\] ' COLORTERM=truecolor TZ=America/New_York; clear`
Enter
Sleep 300ms
Show

Type "daypeek --demo setup"
Enter
Sleep 1s
# 1/4: Things 3 and Obsidian are pre-ticked ("found"); add Todoist
Sleep 800ms
Space
Sleep 600ms
Enter
Sleep 600ms
# Todoist token (fake, hidden)
Type "0123456789abcdef0123456789abcdef"
Enter
Sleep 600ms
# Obsidian vault + daily folder (the detected defaults)
Enter
Sleep 300ms
Enter
Sleep 600ms
# 2/4: an iCal link…
Enter
Sleep 500ms
Type "https://calendar.google.com/calendar/ical/demo/basic.ics"
Enter
Sleep 400ms
Enter
Sleep 600ms
# …and an iCloud account over CalDAV (fake app password; demo calendars)
Up
Sleep 400ms
Enter
Sleep 600ms
Enter
Sleep 900ms
Enter
Sleep 400ms
Type "you@icloud.com"
Enter
Sleep 500ms
Type "abcd-efgh-ijkl-mnop"
Enter
Sleep 900ms
Down
Sleep 300ms
Down
Sleep 300ms
Space
Sleep 500ms
Enter
Sleep 600ms
Enter
Sleep 600ms
# 3/4: weather (suggested from the time zone; type your own to change it)
Sleep 600ms
Type "Evans, Georgia"
Enter
Sleep 400ms
Enter
Sleep 3s
Screenshot docs/screenshot-setup.png
Sleep 1s
