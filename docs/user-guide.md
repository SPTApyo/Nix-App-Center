# User guide

How Nix Software works, in plain words, for people who use it rather than
develop it.

## The idea in one minute

On most systems, an app store installs an app by copying it onto your
computer. NixOS works differently: your whole system is described in text
files, your configuration. To install an app for good, you add its name to
that description, then you apply it. NixOS rebuilds the system from the
description, and you can always go back to the previous version.

Nix Software does that work for you, with three steps:

1. **Try.** The app starts at once, downloaded but not installed. Close it
   and your system is exactly as before.
2. **Add to Configuration.** The app is written into your description.
   Nothing is installed yet.
3. **Apply.** Your system is rebuilt with the new description, and the app
   is installed.

## Where Nix Software writes

Nix Software never edits the files you wrote yourself, except once. It keeps
its own two files next to your configuration:

- nix-software-home.nix, the apps for your user;
- nix-software-system.nix, the apps and settings for the whole computer.

For your system to read them, your configuration must import them. The first
time you add an app, Nix Software shows the exact line it wants to add to
your file, and adds it only if you click Add Import Line. A backup of your
file is written first. If you prefer, Do It Myself copies the line so you can
paste it yourself.

## Your configuration

Nix Software looks for your configuration by itself, usually in /etc/nixos.
The welcome tour and the Preferences show what it found:

- a NixOS flake (a flake.nix file) or a configuration with channels;
- the host, the name of your machine in a flake;
- Home Manager, if you use it.

If what it found is wrong, correct it in Preferences. Point it to the
configuration your computer is really built from: if your /etc/nixos only
imports another folder, keep /etc/nixos. Check with Nix asks Nix itself to
read your configuration, which takes a few seconds and changes nothing.

## Applying

Apply Configuration rebuilds your system. You type your password once. A bar
shows the progress: downloading, then building, then activating. Show Details
opens the full log. You can close the window: the work goes on, and a
notification tells you when it ends.

If applying fails, the message under the bar is the error Nix reported. When
your configuration had an error before you added anything, fix it in your
own files first: Nix Software cannot apply a configuration that does not
build.

## Updates

All your apps come from one snapshot of the Nix package collection, called a
revision. The Updates tab compares your snapshot with the newest one and
lists the apps that would change. Update moves to the newest snapshot and
applies. Apps update together, since they share the snapshot.

## Keeping one app as it is

Sometimes a new version breaks an app you need. From its page, choose:

- **Hold This Version** to keep the version you have now;
- **Choose a Version…** to go back to an earlier one.

A held app stays out of the updates, and its page shows a warning: it gets
no security updates while held. Release lets it follow your configuration
again.

## Build options

Some apps can be built with or without a feature. Build Options on their
page lists those switches. Changing one means the app is built on your
computer instead of downloaded, which can take a long time.

## Storage

Nix keeps every package in one place, the store. The Storage tab shows what
takes space:

- System and User, what your configuration installs;
- Old Generations, previous versions of your system you can go back to;
- Development, what developer tools and your Nix settings keep;
- Garbage, what nothing uses any more.

Clean deletes the garbage, safely. Delete old generations frees more, but you
can no longer go back to them.

## Without NixOS

On another Linux distribution with Nix installed, Nix Software installs apps
for your user only, with nix profile, and updates them with nix profile
upgrade. Nothing of your system is touched.
