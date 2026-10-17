# Security

Nix App Center edits Nix configurations and runs some commands as root through
pkexec (writing files under /etc/nixos, activating a built system, garbage
collection).
Bugs there matter, so please report them privately.

## Reporting

Open a [private security advisory](https://github.com/SPTApyo/nix-app-center/security/advisories/new).
Do not open a public issue. Include what an attacker controls, what they gain,
and steps to reproduce.

Fixes land on main first, then in the next release.

## Supported versions

Only the latest release and main get security fixes.

## Scope

In scope:

- anything run as root: the generated pkexec scripts, the files they write,
  the activation of a system built as the user;
- edits to the user's own configuration (attach, detach);
- text from outside reaching a shell, a Nix file or the UI markup: AppStream,
  Flathub, ODRS reviews, the Devbox index, nixsoftware.org, names from other
  flakes and their lock files;
- deletion of store paths, Flatpaks and AppImages.

Out of scope: vulnerabilities in the packages Nix App Center lets you run,
which come from nixpkgs or the flakes you add, and what you write yourself:
option values, the advanced editor, a custom apply command.
