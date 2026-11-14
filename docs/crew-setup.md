# Crew mode: Aqua as the face of firstmate

In Crew mode you talk to Aqua, and she passes what you say to
[firstmate](https://github.com/kunchenguid/firstmate). Firstmate is a Claude Code
session (the "first mate") that starts and supervises a crew of other Claude Code
sessions, each in its own git worktree, and brings back pull requests or reports.
Aqua reads its replies out loud and shows a badge beside her for each task.

Firstmate supports only macOS and Linux, so on Windows it runs inside WSL2. Setup is
a one-time job of about 15 minutes.

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
| `crew_home`   | `~/firstmate`        | The firstmate folder inside WSL                 |
| `crew_distro` | WSL's default distro | Which WSL distribution to use, e.g. `"Ubuntu"` |

## How it works

The app writes a small helper, `crew.sh`, to its data folder and runs it with
`wsl.exe --exec /bin/bash --login`. It only uses firstmate's documented surfaces for
outside tools:

- `bin/fm-inbox.sh note --json --request-id …` sends a message. The text goes in on
  stdin, and a retried send never queues twice.
- `bin/fm-inbox.sh receipts --all-replies` returns the answers the first mate publishes
  with `fm-inbox.sh reply`. Each message asks for a short spoken answer with an emotion
  tag.
- `state/fleet-ledger.jsonl` lists task events (dispatched, status lines, PR ready,
  merged, cleaned up).

It polls every 4 seconds while Crew mode is on. Replies and alerts from before the app
started are not read out.

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
