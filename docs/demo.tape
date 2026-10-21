# Record an animated GIF of the demo with https://github.com/charmbracelet/vhs
#   brew install vhs && vhs docs/demo.tape
# Uses --demo, so no accounts, tokens, or real data are involved.
Output docs/demo.gif
Set FontSize 15
Set Width 960
Set Height 760
Set Theme "Catppuccin Mocha"
Set TypingSpeed 60ms

Type "today --demo --compact"
Enter
Sleep 2s
Type "today --demo"
Enter
Sleep 5s
