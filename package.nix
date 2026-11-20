{ lib
, nix-gitignore

, rustPlatform
, pkg-config

, zstd
, aws-lc
}:

  let
    toml = builtins.fromTOML (builtins.readFile ./Cargo.toml);
  in
    rustPlatform.buildRustPackage rec
    {
      pname = toml.package.name;
      version = toml.package.version;

      src = nix-gitignore.gitignoreSource [ "*.nix" ] ./.;

      cargoLock.lockFile = ./Cargo.lock;

      buildInputs =
      [
        zstd
        aws-lc
      ];

      nativeBuildInputs = [ pkg-config ];

      meta =
      {
        description = toml.package.description;
        homepage = toml.package.homepage;
        license = builtins.getAttr toml.package.license lib.licensesSpdx;
      };
    }
