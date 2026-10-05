{
  description = "Onyx, a declarative app store for Nix";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-26.05";

  outputs =
    { self, nixpkgs }:
    let
      system = "x86_64-linux";
      pkgs = nixpkgs.legacyPackages.${system};

      # System dependencies shared by the derivation and the devShell: one source
      # of truth (the shell reproduces the build environment exactly).
      nativeBuildInputs = with pkgs; [
        pkg-config
        wrapGAppsHook4
        gettext
      ];
      buildInputs = with pkgs; [
        gtk4
        libadwaita
        glib
        gsettings-desktop-schemas
      ];

      onyx = pkgs.rustPlatform.buildRustPackage {
        pname = "onyx";
        version = "0.1.0";
        src = self; # gitignored files (target/, result, .direnv) are excluded
        cargoLock.lockFile = ./Cargo.lock;
        inherit nativeBuildInputs buildInputs;

        # GTK tests need a display, absent from the Nix sandbox; CI covers them.
        doCheck = false;

        # `cargo build` produces `onyx` (GUI) and `onyx-cli` in $out/bin. Install the
        # desktop entry, metainfo, icons and translations so the app shows up in
        # menus and app stores, and finds its resources at runtime.
        postInstall = ''
          install -Dm644 crates/gui/data/dev.onyx.Onyx.desktop \
            $out/share/applications/dev.onyx.Onyx.desktop
          install -Dm644 crates/gui/data/dev.onyx.Onyx.metainfo.xml \
            $out/share/metainfo/dev.onyx.Onyx.metainfo.xml
          install -Dm644 crates/gui/data/dev.onyx.Onyx.search-provider.ini \
            $out/share/gnome-shell/search-providers/dev.onyx.Onyx.search-provider.ini
          # Lets GNOME Shell start Onyx to answer searches.
          install -d $out/share/dbus-1/services
          printf '[D-BUS Service]\nName=dev.onyx.Onyx\nExec=%s --gapplication-service\n' \
            "$out/bin/onyx" > $out/share/dbus-1/services/dev.onyx.Onyx.service
          mkdir -p $out/share/icons
          cp -r crates/gui/data/icons/hicolor $out/share/icons/
          for po in po/*.po; do
            lang=$(basename "$po" .po)
            install -d $out/share/locale/$lang/LC_MESSAGES
            msgfmt "$po" -o $out/share/locale/$lang/LC_MESSAGES/onyx.mo
          done
        '';

        preFixup = ''
          gappsWrapperArgs+=(--set ONYX_LOCALEDIR "$out/share/locale")
        '';

        meta = with pkgs.lib; {
          description = "Declarative app store for Nix (try first with nix run, then keep)";
          license = licenses.gpl3Plus;
          mainProgram = "onyx";
          platforms = platforms.linux;
        };
      };
    in
    {
      packages.${system} = {
        default = onyx;
        onyx = onyx;
      };

      # `nix run .` opens the window; `nix run .#onyx-cli -- search …` runs the CLI.
      apps.${system} = {
        default = {
          type = "app";
          program = "${onyx}/bin/onyx";
        };
        onyx-cli = {
          type = "app";
          program = "${onyx}/bin/onyx-cli";
        };
      };

      devShells.${system}.default = pkgs.mkShell {
        inherit nativeBuildInputs buildInputs;
        packages = with pkgs; [
          rustc
          cargo
          rust-analyzer
          clippy
          rustfmt
        ];
      };
    };
}
