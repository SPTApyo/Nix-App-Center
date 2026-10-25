# Keeps an app, applies, then goes back, with a real nixos-rebuild in the
# VM. Offline: the build dependencies are in the image. Slow, so it is not
# part of the nixpkgs test.
{ hostPkgs, ... }:

{
  name = "nix-app-center-apply";

  # The installation profile adds an overlay.
  node.pkgsReadOnly = false;

  nodes.machine =
    {
      lib,
      pkgs,
      modulesPath,
      ...
    }:
    {
      imports = [
        "${modulesPath}/profiles/installation-device.nix"
        "${modulesPath}/profiles/base.nix"
      ];
      programs.nix-app-center.enable = true;
      nix.settings = {
        substituters = lib.mkForce [ ];
        hashed-mirrors = null;
        connect-timeout = 1;
      };
      system.includeBuildDependencies = true;
      system.extraDependencies = [
        pkgs.grub2
        pkgs.hello
      ];
      system.switch.enable = true;
      virtualisation = {
        cores = 2;
        memorySize = 4096;
      };
    };

  testScript =
    { nodes, ... }:
    let
      # The base system drops the module, the store keeps the CLI.
      cli = "${nodes.machine.programs.nix-app-center.package}/bin/nix-app-center-cli";
      configFile = hostPkgs.writeText "configuration.nix" ''
        { ... }: {
          imports = [
            ./hardware-configuration.nix
            <nixpkgs/nixos/modules/testing/test-instrumentation.nix>
          ];
          boot.loader.grub = {
            enable = true;
            device = "/dev/vda";
            forceInstall = true;
          };
          documentation.enable = false;
        }
      '';
    in
    ''
      machine.start()
      machine.succeed("udevadm settle")
      machine.wait_for_unit("multi-user.target")
      machine.succeed("nixos-generate-config")
      machine.copy_from_host("${configFile}", "/etc/nixos/configuration.nix")

      with subtest("Base system"):
          machine.succeed("nixos-rebuild switch")
          machine.fail("hello")

      with subtest("Keep an app and apply"):
          machine.succeed("${cli} attach --commit")
          machine.succeed("${cli} keep hello")
          machine.succeed("${cli} apply >&2")
          machine.succeed("hello")
          print(machine.succeed("${cli} history"))

      with subtest("Go back"):
          base = machine.succeed("readlink /nix/var/nix/profiles/system").strip()
          number = int(base.split("-")[1])
          machine.succeed("${cli} unkeep hello")
          machine.succeed("${cli} apply >&2")
          machine.fail("hello")
          out = machine.succeed(f"${cli} go-back {number} 2>&1")
          assert "configuration restored" in out, out
          machine.succeed("hello")
          machine.succeed("${cli} list | grep -q '^hello'")
          # The next apply keeps the state gone back to.
          machine.succeed("${cli} apply >&2")
          machine.succeed("hello")
    '';
}
