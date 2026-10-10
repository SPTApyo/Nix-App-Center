# Packaging

How Nix Software is built and released, and how it gets into nixpkgs so
people install it with pkgs.nix-software, without a flake input or a channel.

## One recipe

nix/package.nix is the derivation exactly as it goes into nixpkgs
(pkgs/by-name/ni/nix-software/package.nix). default.nix builds that same
recipe from the local tree: it swaps the release tarball for the sources and
the cargoHash for Cargo.lock. flake.nix imports default.nix. So every CI run
of nix build tests the nixpkgs recipe: dependencies, install steps, metadata
and the core tests in the Nix sandbox.

Change nix/package.nix, never a copy of it.

Only the app icon goes to share/icons. The other icons have names that other
packages ship too (GNOME Software's category and action icons), which would
collide in a profile. They live in share/nix-software/icons, and the app adds
that folder to its icon search path.

## Releasing

1. Bump version in crates/*/Cargo.toml and nix/package.nix, then run
   cargo update --workspace.
2. Add a release entry to
   crates/gui/data/io.github.sptapyo.NixSoftware.metainfo.xml.
3. Commit (chore: release vX.Y.Z), tag vX.Y.Z and push the tag.
4. Write the GitHub release notes from the commits since the last tag.

## Getting into nixpkgs

Each release after the first only needs the version and the two hashes
updated, which nixpkgs' update bot can do. The first time:

1. Fork NixOS/nixpkgs and branch from master.
2. Add yourself to maintainers/maintainer-list.nix, in alphabetical order:

   ```nix
   sptapyo = {
     name = "SPTApyo";
     github = "SPTApyo";
     githubId = 172824481;
   };
   ```

   Commit it alone: maintainers: add sptapyo.
3. Copy nix/package.nix to pkgs/by-name/ni/nix-software/package.nix. Fill the
   hashes: build once with lib.fakeHash, then paste the hash Nix prints, for
   src first and cargoHash second:

   ```bash
   nix-build -A nix-software
   ```

4. Check it the way reviewers will:

   ```bash
   nixfmt pkgs/by-name/ni/nix-software/package.nix
   nix-build -A nix-software
   ./result/bin/nix-software
   nix-shell -p nixpkgs-review --run "nixpkgs-review wip"
   ```

5. Commit as nix-software: init at 0.1.0 and open the pull request against
   master, filling the template (tested on NixOS, built with sandboxing).

Once merged, the package reaches nixos-unstable within a few days, and the
next stable NixOS release when it branches off. Users then write
pkgs.nix-software like any other package.
