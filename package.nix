{ lib, linkFarm, nix, rustPlatform }:
  let
    toml = builtins.fromTOML (builtins.readFile ./Cargo.toml);
  in
    rustPlatform.buildRustPackage rec
    {
      pname = toml.package.name;
      version = toml.package.version;

      src = linkFarm "lxddns-${version}-src"
      [
        { name = "src"; path = ./src; }
        { name = "Cargo.toml"; path = ./Cargo.toml; }
        { name = "Cargo.lock"; path = ./Cargo.lock; }
        { name = "COPYING"; path = "COPYING"; }
      ];

      cargoLock.lockFile = ./Cargo.lock;

      auditable = true; # TODO: remove when this is the default

      passthru =
      {
        tests =
        {
          inherit nix;
        };
      };

      meta = with lib;
      {
        description = toml.package.description;
        homepage = toml.package.homepage;
        license = builtins.filter (l: builtins.hasAttr "spdxId" l && l.spdxId == toml.package.license) (builtins.attrValues licenses);
      };
    }
