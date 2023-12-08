{
  description = "lxddns package and NixOS module";

  inputs =
  {
    # please override these inputs when using the flake and point them to
    #  1. the version you're using
    #  2. ideally your non-GitHub mirror
    nixpkgs.url = "git+https://git.shell.bsocat.net/nixpkgs?ref=nixos-23.11";
    systems.url = "git+https://git.shell.bsocat.net/nix-systems";
    flake-utils.url = "git+https://git.shell.bsocat.net/flake-utils";
    flake-utils.inputs.systems.follows = "systems";
  };

  outputs = { self, nixpkgs, flake-utils, ... }:
  {
    packages = flake-utils.lib.eachDefaultSystem (system:
        let
          # pkgs for current system
          pkgs = import nixpkgs
          {
            inherit system;
            overlays = [ self.overlays.default ];
          };
        in
          {
            lxddns = pkgs.lxddns;
            lxddns-http = pkgs.lxddns-http;
            lxddns-amqp = pkgs.lxddns-amqp;
            default = pkgs.lxddns;
          }
      );
    checks = flake-utils.lib.eachDefaultSystem (system:
        let
          # pkgs for current system
          pkgs = import nixpkgs
          {
            inherit system;
            overlays = [ self.overlays.default ];
          };
        in
          {
            lxddns = pkgs.lxddns;
            lxddns-http = pkgs.lxddns-http;
            lxddns-amqp = pkgs.lxddns-amqp;
          }
      );
    nixosModules = rec
    {
      lxddns = ./module.nix;
      default = lxddns;
    };
    overlays = rec
    {
      lxddns = final: prev:
        let
          lxddns = prev.callPackage ./package.nix {};
          featureOverrideAttrs = features:
          {
            cargoBuildNoDefaultFeatures = true;
            cargoBuildFeatures = features;
            cargoCheckNoDefaultFeatures = true;
            cargoCheckFeatures = features;
          };
        in
          {
            lxddns = lxddns;
            lxddns-http = lxddns.overrideAttrs (featureOverrideAttrs [ "http" ]);
            lxddns-amqp = lxddns.overrideAttrs (featureOverrideAttrs [ "amqp" ]);
          };
      default = lxddns;
    };
  };
}
