let
  lock = builtins.fromJSON (builtins.readFile ./flake.lock);
  get = repo: builtins.fetchTree lock.nodes.${repo}.locked;
  defaultArgs =
  {
    system = builtins.currentSystem;
    nixpkgs = get "nixpkgs";
    nix-systems = get "nix-systems";
    buildTarget = "pkgs"; # pkgs | flake | hydraJobs
    self = { outPath = ./.; };
  };

  output = { buildTarget, ... }@args:
    let
      lib = import "${args.nixpkgs}/lib";
      overlay = import ./overlay.nix;
      module = ./module.nix;
      pkgsFor = system: import args.nixpkgs { inherit system; overlays = [ overlay ]; };
    in
      builtins.getAttr buildTarget (lib.fix (self:
      {
        flake =
        {
          inherit (self) hydraJobs;
          checks = lib.genAttrs (import args.nix-systems) (system: let inherit (pkgsFor system) lib pkgs; in
          {
            # minor workaround for not having aarch64-linux machines with kvm support
            nixos-incus = lib.optionalAttrs (system == "x86_64-linux") (pkgs.nixosTest (import ./test.nix
            {
              inherit pkgs module overlay;
            }));
            # only run linting once, on x86_64-linux (preferrably aarch64 but it's less common)
            lint-deadnix = pkgs.lib.optionalAttrs (system == "x86_64-linux") (pkgs.runCommand "deadnix" {} "${pkgs.deadnix}/bin/deadnix --fail -- ${args.self} | tee /dev/stderr > $out");
            lint-statix = pkgs.lib.optionalAttrs (system == "x86_64-linux") (pkgs.runCommand "statix" {} "${pkgs.statix}/bin/statix check --config ${args.self}/statix.toml -- ${args.self} | tee /dev/stderr > $out");
          });
          packages = lib.genAttrs (import args.nix-systems) (system: let pkgs = pkgsFor system; in
          {
            inherit (pkgs) lxddns lxddns-http lxddns-amqp;
            default = pkgs.lxddns;
          });
          nixosModules =
          {
            lxddns = module;
            default = module;
          };
          overlays =
          {
            lxddns = overlay;
            default = overlay;
          };
        };
        hydraJobs = { inherit (self.flake) packages checks; };
        pkgs = pkgsFor args.system;
      }));

in
  { ... }@args: output (defaultArgs // args)
