# Crew mode: Aqua as the face of firstmate

In Crew mode you talk to Aqua, and she passes what you say to firstmate. Firstmate is a
Claude Code session (the "first mate") that starts and supervises a crew of other Claude
Code sessions, each in its own git worktree, and brings back pull requests or reports.
Aqua reads its replies out loud and shows a badge beside her for each task.

There are two ways to run firstmate on Windows:

- **Native, under Git Bash (the default).** Use
  [firstmate-for-windows](https://github.com/EvanBatten/firstmate-for-windows), a port
  that runs under Git Bash with [Herdr](https://herdr.dev) as the crew's terminal. Follow
  [Native Windows](#native-windows-git-bash) below.
- **WSL2.** Run upstream [firstmate](https://github.com/kunchenguid/firstmate) inside
  Ubuntu, the platform it's built for. Set `"crew_shell": "wsl"` in settings.json and
  follow steps 1 to 4 below.

## She is the first mate

By default the app runs firstmate's first mate itself: one long-running headless Claude
Code session in the firstmate folder (`claude -p --input-format stream-json`), with her
character added on top of firstmate's own instructions. Your messages go straight in and
her answers stream back, like Claude Code mode. When firstmate wakes her for crew news
(a decision, a failure, a PR), she says it out loud; wakes that need nothing from you stay
quiet.

- The session starts when Crew mode is turned on (or at launch), because firstmate's
  startup takes a minute or so. It's resumed across app restarts (`crew_session` in
  settings.json).
- Don't also run `claude` in the firstmate folder yourself: only one first mate can hold
  firstmate's lock. **Open crew terminal** only opens Herdr to watch the crew; it no
  longer starts a first mate there.
- Set `"crew_direct": false` to go back to passing messages to a first mate you run in
  Herdr yourself (the older setup described below).

## Native Windows (Git Bash)

1. Install the tools listed in firstmate-for-windows'
   [install guide](https://github.com/EvanBatten/firstmate-for-windows/blob/main/platform/windows/INSTALL.md):
   Developer Mode, Git for Windows, PowerShell 7, gh, Node.js, jq, Claude Code, Herdr and
   Treehouse. Also install Python 3 (`winget install --id Python.Python.3.12 -e`), which
   firstmate's inbox needs.
2. Clone it with symlinks on, for example into your projects folder:
   ```powershell
   git clone -c core.symlinks=true https://github.com/EvanBatten/firstmate-for-windows "$env:USERPROFILE\Documents\projects\firstmate"
   ```
   Then set `"crew_home": "~/Documents/projects/firstmate"` in settings.json (`~` is your
   Windows user folder in Git Bash).
3. Recommended: `auto` permission mode for crewmates (see step 3 below). In Git Bash:
   `echo auto > ~/Documents/projects/firstmate/config/claude-permission-mode`.
4. Do the guide's one-time [launch steps](https://github.com/EvanBatten/firstmate-for-windows/blob/main/platform/windows/INSTALL.md#launch)
   (Herdr's shell and the `claude` profile line).
5. **Open crew terminal** in the tray starts Herdr in the firstmate folder and, when
   the first mate isn't already running, types `claude` into its first pane to start it.
   (If that doesn't happen, type `claude` there yourself.) Then continue with step 5 below.

The rest of this guide is the WSL route.

## 1. Install WSL2 and Ubuntu

In PowerShell **as administrator**:

```powershell
wsl --install -d Ubuntu
```

Restart if asked, then open **Ubuntu** from the Start menu and create a Linux user
name and password.

## 2. Install the tools inside Ubuntu

In the Ubuntu window:

```sh
sudo apt update
sudo apt install -y git tmux python3 jq curl gh
curl -fsSL https://claude.ai/install.sh | bash
exec bash -l            # picks up ~/.local/bin, where claude was installed
gh auth login           # GitHub.com, HTTPS, log in with a browser
claude                  # sign in once, then type /exit
```

Firstmate checks the rest of its toolchain on first launch and asks before it installs
anything.

## 3. Get firstmate

```sh
git clone https://github.com/kunchenguid/firstmate ~/firstmate
```

**Recommended:** have crewmates ask Claude Code's safety classifier instead of skipping
every permission check. Out of the box firstmate starts each crewmate with
`--dangerously-skip-permissions`.

```sh
echo auto > ~/firstmate/config/claude-permission-mode
```

If your Claude plan doesn't support `auto` mode, firstmate refuses to start a crewmate
and says so. Delete the file to go back to the default.

## 4. Start the first mate

In Aqua's tray menu choose **Open crew terminal**. A console window opens a tmux
session called `firstmate`, with Claude Code running in `~/firstmate`. Each crewmate
gets its own tmux window there, so you can always watch or step in. (Close the window
and choose **Open crew terminal** again to reattach. The crew keeps running.)

The first time, tell it about your project and how changes should land, for example:

> add github.com/rajieluo/desktop-assistant as a project in direct-PR mode

`direct-PR` means crewmates open pull requests and never merge on their own. Don't add
`+yolo` unless you want firstmate to merge by itself.

## 5. Turn on Crew mode

Tick **Crew mode (firstmate)** in the tray. The text box gets a blue border.

- **Talk to her as usual**, typing or with the mic. Each message is queued for the
  first mate with `bin/fm-inbox.sh note`. She says right away that she passed it on,
  then reads out the first mate's answer when it arrives.
- **Badges** down her right side show each task: blue for working, amber for
  waiting on you, green for PR ready or merged, red for failed. Click one for its
  latest status line, a link to its PR and a button to open the terminal.
- **She speaks up by herself only when something needs you**: a task needs a decision
  or is blocked, a task failed, a PR is ready, or a PR was merged.

To answer a decision, reply through Aqua ("for fix login, keep the old API") or type in
the firstmate terminal.

The first time Crew mode runs, the app creates `~/firstmate/config/fleet-ledger`. That
file turns on firstmate's task log (`state/fleet-ledger.jsonl`), which is where the
badges come from.

## Settings

In `%APPDATA%\com.rajieluo.desktopassistant\settings.json`:

| Setting       | Default              | Meaning                                         |
| ------------- | -------------------- | ----------------------------------------------- |
| `crew_shell`  | `git-bash` when Git for Windows is installed, else `wsl` | How the app reaches firstmate |
| `crew_home`   | `~/firstmate`        | The firstmate folder (in Git Bash or WSL terms) |
| `crew_direct` | `true` | She is the first mate (see above); `false` passes notes to one you run |
| `crew_permission_mode` | `auto` | Claude Code permission mode for the first mate |
| `crew_session` | set by the app | The first mate's session, resumed across restarts |
| `crew_distro` | an Ubuntu distro if there is one | Which WSL distribution to use, e.g. `"Ubuntu"`. Docker Desktop's distros are always skipped. |

## How it works

The app writes a small helper, `crew.sh`, to its data folder and runs it with Git
Bash's `bash.exe --login`, or with `wsl.exe --exec /bin/bash --login` in WSL mode. It only uses firstmate's documented surfaces for
outside tools:

- `bin/fm-inbox.sh note --json --request-id …` sends a message. The text goes in on
  stdin, and a retried send never queues twice.
- `bin/fm-inbox.sh receipts --all-replies` returns the answers the first mate publishes
  with `fm-inbox.sh reply`. Each message asks for a short spoken answer with an emotion
  tag.
- `state/fleet-ledger.jsonl` lists task events (dispatched, status lines, PR ready,
  merged, cleaned up).

- `state/avatar-say/*.txt` holds announcements the first mate wants said without being
  asked (format in the README's Crew mode section). Each file is said once and then
  deleted.

It polls every 4 seconds while Crew mode is on. Replies and alerts from before the app
started are not read out. Announcements are, because each one is written on purpose.

## Troubleshooting

- **"I can't find firstmate at ~/firstmate"**: clone it (step 3), or set `crew_home`.
- **"WSL has no Linux distribution yet"**: run step 1.
- **"Claude Code isn't installed inside WSL"**: install it in Ubuntu (step 2). The
  Windows copy of Claude Code doesn't count.
- **She passes messages on but never answers**: open the crew terminal and check the
  first mate is running. It answers a note at its next check, which can take a while
  when it's busy. It may also answer only in its terminal; ask it to "answer avatar
  notes with fm-inbox.sh reply".
- **"saved it but couldn't wake it"**: the note is kept, and the first mate reads it the
  next time it starts.
- The app log (`%LOCALAPPDATA%\com.rajieluo.desktopassistant\logs\`) has a line for
  each failed poll.
