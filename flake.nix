{
  description = "lxddns package and NixOS module";

  inputs =
  {
    # please override these inputs when using the flake and point them to
    #  1. the version you're using
    #  2. ideally your non-GitHub mirror
    nixpkgs = { url = "git+https://git.shell.bsocat.net/nixpkgs?ref=nixos-25.05"; flake = false; };
    nix-systems = { url = "git+https://git.shell.bsocat.net/nix-systems-default-linux"; flake = false; };
  };

  outputs = args: import ./default.nix (args // { system = "x86_64-linux"; buildTarget = "flake"; });
}
