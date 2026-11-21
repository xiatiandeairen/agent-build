# Records the raw demo video that docs/demo/zoom.py turns into demo.gif and demo.mp4.
# Run docs/demo/record.sh from the repo root; it prepares /tmp/agb-demo and the background first.
# The builds are real: xcodebuild on tests/fixtures/SampleApp, first cold, then from a second worktree.
# Recorded at 2x so zoom.py can zoom in up to 2x without blur.
Output "/tmp/agb-demo/raw.mp4"
Set Shell bash
Set FontSize 28
Set Width 2880
Set Height 1620
Set Padding 32
Set Margin 80
Set MarginFill "/tmp/agb-demo/bg.png"
Set WindowBar Colorful
Set WindowBarSize 60
Set BorderRadius 20
Set Theme "Catppuccin Mocha"
Set TypingSpeed 25ms
# VHS drops frames (and speeds the video up) when it cannot capture 2x frames in time; 15 fps keeps real time.
Set Framerate 15
Set WaitTimeout 120s
Set WaitPattern /\$\s*$/

Hide
Type "bash --norc" Enter
Type "source /tmp/agb-demo/env && cd /tmp/agb-demo/app && clear" Enter
Wait
Show
Sleep 1s

# First build of the project: cold.
Type "agb xcodebuild build -scheme App -destination 'generic/platform=iOS Simulator'" Enter
Wait
Sleep 2.5s

# Another worktree of the same project: same slot, so only what changed is rebuilt.
Type "cd ../app-feature" Enter
Type "agb xcodebuild build -scheme App -destination 'generic/platform=iOS Simulator'" Enter
Wait
Sleep 2.5s

# A mistake: the error comes back with this worktree's path, ready to jump to.
Type "sed -i '' 's/Greeting.text/Greeting.title/' App/App.swift" Enter
Type "agb xcodebuild build -scheme App -destination 'generic/platform=iOS Simulator'" Enter
Wait
Sleep 4s
