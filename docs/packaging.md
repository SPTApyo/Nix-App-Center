# Packaging

How Nix App Center is built and released, and how it gets into nixpkgs so
people install it with pkgs.nix-app-center, without a flake input or a channel.

## One recipe

nix/package.nix is the derivation exactly as it goes into nixpkgs
(pkgs/by-name/ni/nix-app-center/package.nix). default.nix builds that same
recipe from the local tree: it swaps the release tarball for the sources and
the cargoHash for Cargo.lock. flake.nix imports default.nix. So every CI run
of nix build tests the nixpkgs recipe: dependencies, install steps, metadata
and the core tests in the Nix sandbox.

Change nix/package.nix, never a copy of it. The same goes for the NixOS
module, nix/module.nix (nixos/modules/programs/nix-app-center.nix in nixpkgs),
and its test, nix/test.nix (nixos/tests/nix-app-center.nix). The flake runs
that test as checks.<system>.module, and CI builds on x86_64-linux and
aarch64-linux.

The recipe's passthru carries what nixpkgs reviewers look for:
tests.version (the CLI answers --version), tests.module (the NixOS test)
and updateScript, so the update bot follows new tags.

Only the app icon goes to share/icons. The other icons have names that other
packages ship too (GNOME Software's category and action icons), which would
collide in a profile. They live in share/nix-app-center/icons, and the app adds
that folder to its icon search path.

## Releasing

Versions follow SemVer, bumped once per release, not per commit:

- last digit (0.1.x): fixes and small improvements;
- middle digit (0.x.0): a new feature users can see;
- 1.0.0: once the app is stable and in nixpkgs.

Numbers never wrap: 0.1.9 is followed by 0.1.10.

1. Bump version in crates/*/Cargo.toml and nix/package.nix, then run
   cargo update --workspace.
2. Add a release entry to
   crates/gui/data/io.github.sptapyo.NixAppCenter.metainfo.xml.
3. Commit (chore: release vX.Y.Z), tag vX.Y.Z and push the tag.
4. Write the GitHub release notes from the commits since the last tag.

## Getting into nixpkgs

Each release after the first only needs the version and the two hashes
updated, which nixpkgs' update bot can do.

Adding the module to nixpkgs also needs:

- nixos/modules/programs/nix-app-center.nix: nix/module.nix without its two
  comment lines.
- nixos/modules/module-list.nix: ./programs/nix-app-center.nix, in order.
- nixos/tests/nix-app-center.nix: nix/test.nix without its comment line.
- nixos/tests/all-tests.nix: nix-app-center = runTest ./nix-app-center.nix;
- nixos/doc/manual/release-notes, current release file: one line under new
  modules, linking programs.nix-app-center.

The first time:

1. Fork NixOS/nixpkgs and branch from master.
2. Add yourself to maintainers/maintainer-list.nix, in alphabetical order:

   ```nix
   SPTApyo = {
     name = "SPTApyo";
     github = "SPTApyo";
     githubId = 172824481;
   };
   ```

   Commit it alone: maintainers: add SPTApyo.
3. Copy nix/package.nix to pkgs/by-name/ni/nix-app-center/package.nix. Fill the
   hashes: build once with lib.fakeHash, then paste the hash Nix prints, for
   src first and cargoHash second:

   ```bash
   nix-build -A nix-app-center
   ```

4. Check it the way reviewers will:

   ```bash
   nixfmt pkgs/by-name/ni/nix-app-center/package.nix
   nix-build -A nix-app-center
   ./result/bin/nix-app-center
   nix-shell -p nixpkgs-review --run "nixpkgs-review wip"
   ```

5. Commit as nix-app-center: init at 0.2.1 and open the pull request against
   master, filling the template (tested on NixOS, built with sandboxing).

Once merged, the package reaches nixos-unstable within a few days, and the
next stable NixOS release when it branches off. Users then write
pkgs.nix-app-center like any other package.
