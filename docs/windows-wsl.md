# Windows development with WSL 2

Exitbind's supported Windows path is Ubuntu under WSL 2. Exitbind runs as the
published Linux binary inside the distribution; it is not a native Windows
executable. Keep Codex or Claude Code, `git`, `tmux`, Exitbind, and the project
inside the same WSL distribution.

Native PowerShell/CMD execution, Windows-path state roots, and a native Windows
away runner are not supported. The WSL process also cannot continue while the
Windows machine is asleep, shut down, or rebooting.

## Install WSL 2

On Windows 10 version 2004 or later, or Windows 11, open an administrator
PowerShell and run:

```powershell
wsl.exe --install --distribution Ubuntu
```

Restart Windows if requested, launch Ubuntu once, and create the Linux user
when prompted. Confirm that Ubuntu uses WSL 2:

```powershell
wsl.exe --list --verbose
```

## Install Exitbind inside Ubuntu

To install the published `v0.31.21` release, open the Ubuntu terminal.
Keep projects under the Linux home directory, not under `/mnt/c`, so Linux path,
permission, and filesystem behavior remain the ones Exitbind tests.

```sh
sudo apt-get update
sudo apt-get install -y curl git tmux
curl -fsSL https://github.com/PANIC-ANGEL-SCIENCE/exitbind/releases/latest/download/install.sh | sh
export PATH="$HOME/.local/bin:$PATH"

mkdir -p "$HOME/projects"
exitbind init --mode portable --root "$HOME/projects/first-exitbind-project"
exitbind check --config "$HOME/projects/first-exitbind-project/exitbind.json"
```

Persist `~/.local/bin` in `PATH` using the shell's normal startup file before
starting Codex or Claude Code from Ubuntu.

## Agent and away-runner boundary

Install and launch the coding-agent CLI inside the same Ubuntu distribution.
The optional `exitbind away` path additionally requires `tmux` there. It can
survive a terminal, editor, or SSH client disconnect while Ubuntu and Windows
continue running; it cannot survive Windows sleep, shutdown, reboot, or
`wsl.exe --shutdown`.

From PowerShell, enter the default distribution with `wsl.exe`. Windows tools
can browse the project through `\\wsl$`, but run Exitbind and its agent host
from the Linux side.

## Evidence boundary

The release CI is configured to build the locked Linux stable candidate,
transfer that exact workflow artifact to Ubuntu under WSL 2 on GitHub's
`windows-2025` runner, and exercise
the installed release through `init`, `check`, `brief`, and `run` state
creation/inspection. Existing Linux CI owns the deeper Rust and real-tmux
coverage. This evidence does not claim native Windows filesystem, process,
hook-mutation, or binary support.
