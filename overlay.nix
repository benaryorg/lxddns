let
  genOverride = { features ? [], mainProgram ? "lxddns" }: { meta ? {}, ... }:
  {
    cargoBuildNoDefaultFeatures = features != [];
    cargoBuildFeatures = features;
    cargoCheckNoDefaultFeatures = features != [];
    cargoCheckFeatures = features;
    meta = meta // { inherit mainProgram; };
  };
  genOverrideShort = type: genOverride { features = [ type ]; mainProgram = "lxddns-${type}"; };
in
  final: _prev:
  {
    lxddns = final.callPackage ./package.nix {};
    lxddns-http = final.lxddns.overrideAttrs (genOverrideShort "http");
    lxddns-amqp = final.lxddns.overrideAttrs (genOverrideShort "amqp");
  }
