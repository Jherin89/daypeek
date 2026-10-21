# The `daypeek setup` wizard, in demo mode (fake input, nothing is saved).
#   cargo build --release && vhs docs/setup.tape
Output docs/setup.gif

Set Shell "bash"
Set FontFamily "JetBrains Mono"
Set FontSize 14
Set Width 900
Set Height 1000
Set Padding 18
Set Theme "Catppuccin Mocha"
Set TypingSpeed 40ms
Set Framerate 15

Hide
Type `unset NO_COLOR FORCE_COLOR; export PATH="$PWD/target/release:$PATH" PS1='\[\e[32m\]❯\[\e[0m\] ' COLORTERM=truecolor; clear`
Enter
Sleep 300ms
Show

Type "daypeek --demo setup"
Enter
Sleep 1s
# 1/4: Todoist, Things 3, Obsidian
Space
Sleep 200ms
Down 5
Space
Sleep 200ms
Down 2
Space
Sleep 600ms
Enter
Sleep 600ms
# Todoist token (fake, hidden)
Type "0123456789abcdef0123456789abcdef"
Enter
Sleep 600ms
# Obsidian vault + daily folder (defaults)
Enter
Sleep 300ms
Type "Daily"
Enter
Sleep 600ms
# 2/4: one calendar
Type "https://calendar.google.com/calendar/ical/demo/basic.ics"
Enter
Sleep 400ms
Enter
Sleep 400ms
Enter
Sleep 600ms
# 3/4: weather
Type "Evans, Georgia"
Enter
Sleep 400ms
Enter
Sleep 3s
Screenshot docs/screenshot-setup.png
Sleep 1s
