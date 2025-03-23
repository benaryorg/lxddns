{
  description = "lxddns package and NixOS module";

  inputs =
  {
    # please override these inputs when using the flake and point them to
    #  1. the version you're using
    #  2. ideally your non-GitHub mirror
    nixpkgs.url = "git+https://git.shell.bsocat.net/nixpkgs?ref=nixos-24.11";
    systems.url = "git+https://git.shell.bsocat.net/nix-systems-default-linux";
    flake-utils.url = "git+https://git.shell.bsocat.net/flake-utils";
    flake-utils.inputs.systems.follows = "systems";
  };

  outputs = { self, nixpkgs, flake-utils, ... }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        # pkgs for current system
        pkgs = nixpkgs.legacyPackages.${system}.extend self.overlays.default;
      in
        {
          packages =
          {
            lxddns = pkgs.lxddns;
            lxddns-http = pkgs.lxddns-http;
            lxddns-amqp = pkgs.lxddns-amqp;
            default = pkgs.lxddns;
          };
          checks =
          {
            # minor workaround for not having aarch64-linux machines with kvm support
            nixos-incus = pkgs.lib.optionalAttrs (system == "x86_64-linux") (pkgs.nixosTest (import ./test.nix
            {
              inherit pkgs;
              module = self.outputs.nixosModules.lxddns;
              overlay = self.outputs.overlays.lxddns;
            }));
            # only run linting once, on x86_64-linux (preferrably aarch64 but it's less common)
            lint-deadnix = pkgs.lib.optionalAttrs (system == "x86_64-linux") (pkgs.runCommand "lxddns-deadnix" { meta.hydraPlatforms = [ "x86_64-linux" ]; } "${pkgs.deadnix}/bin/deadnix --fail -- ${self} | tee /dev/stderr > $out");
            lint-statix = pkgs.lib.optionalAttrs (system == "x86_64-linux") (pkgs.runCommand "lxddns-statix" { meta.hydraPlatforms = [ "x86_64-linux" ]; } "${pkgs.statix}/bin/statix check --config ${self}/statix.toml -- ${self} | tee /dev/stderr > $out");
          };
        }
    )
    //
    {
      nixosModules = rec
      {
        lxddns = ./module.nix;
        default = lxddns;
      };
      overlays = rec
      {
        lxddns = import ./overlay.nix;
        default = lxddns;
      };
      hydraJobs = { inherit (self) packages checks; };
    };
}
