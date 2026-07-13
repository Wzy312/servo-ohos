# Contributing to this Fork

This repository is a fork of servo, mainly carrying additional OpenHarmony related patches to achieve a
faster development velocity.
Patches that comply with upstream rules and are stable, should preferably be submitted upstream, to help keep this fork maintainable.
Patches that are blocked upstream, and improve the experience on ohos can be merged here, to ease sharing between ohos contributors.

The `main` branch follows `upstream/main` and carries no patches. 
The `ohos-main` contains `main` + our patches and will be rebased regularly (via force-push).
Previous states will be preserved by tagging commits before / after syncing.
`base/<date>` tags mark the base state of `main` on a given date of a sync.
`pre-sync/<date>` tags mark the last commit on `ohos-main` before a rebase onto main.
`sync/<date>` marks the last commit on `ohos-main` after a rebase onto main.


The rules in this fork are roughly:

- If something is trivial, fix is upstream
- If something is OpenHarmony / arkweb specific, it can be merged here, however it should be mostly additive to minimize rebasing effort.
- Each commit should have a `Validation: Test by doing XYZ` comment in the commit message, explaining which tests are relevant to ensure the commit
  continues working as intended during rebasing.
- The CI matrix is reduced (linux + ohos) to limit overhead, but try to not break other platforms. If regressions are discovered for other platforms, the patch should be fixed.
- AI assisted contributions, are allowed in this fork but must follow these rules:
  - Must be disclosed with an `AI-assisted` trailer in the commit message, and further information on how AI was used.
  - This mainly targets code generation, local usage to do research or review (without modifying) does not require disclosure.
  - The author remains responsible for their patch. Don't submit patches you don't understand.
  - Low quality PRs may be rejected and closed.
  - The PR description, PR comments and any discussion with reviewers must be fully written by a human. Respect the reviewers time.
    Any AI generated prose must be clearly marked as such, and there is no expectation that a reviewer will read it.
- Otherwise, this is very much an experiment, so expect changes.


Please also view the [upstream Contributing to Servo Guide](https://book.servo.org/contributing/getting-started).
