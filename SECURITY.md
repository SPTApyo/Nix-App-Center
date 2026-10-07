# Security

Nix Software edits Nix configurations and runs some commands as root through
pkexec (writing files under /etc/nixos, nixos-rebuild, garbage collection).
Bugs there matter, so please report them privately.

## Reporting

Open a [private security advisory](https://github.com/SPTApyo/nix-software/security/advisories/new).
Do not open a public issue. Include what an attacker controls, what they gain,
and steps to reproduce.

Fixes land on main first, then in the next release.

## Supported versions

Only the latest release and main get security fixes.

## Scope

In scope:

- anything run as root: the generated pkexec scripts, the files they write;
- edits to the user's own configuration (attach, detach);
- text from the network (AppStream, Flathub, ODRS reviews) reaching a shell,
  a Nix file or the UI markup;
- deletion of store paths (removing tried apps, clean-ups).

Out of scope: vulnerabilities in the packages Nix Software lets you run.
They come from nixpkgs.
