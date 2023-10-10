{ lib, stdenv, nix, rustPlatform }:
  let
    toml = builtins.fromTOML (builtins.readFile ./Cargo.toml);
  in
    rustPlatform.buildRustPackage rec
    {
      pname = toml.package.name;
      version = toml.package.version;

      src = ./.;

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
