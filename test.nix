{ overlay, module, pkgs }:
{
  name = "lxddns-nixos-vm-test";
  nodes.host = { pkgs, config, ... }:
  {
    imports = [ module { config.nixpkgs.overlays = [ overlay ]; } ];
    config =
    {
      security.pki.certificateFiles = [ config.services.lxddns-responder.http.tls-cert ];
      services =
      {
        lxddns-responder =
        {
          enable = true;
          virt-command = "${config.virtualisation.incus.package}/bin/incus";
          http =
          {
            tls-cert = builtins.toString (pkgs.writers.writeText "snakeoil-cert.pem"
            ''
              -----BEGIN CERTIFICATE-----
              MIICQjCCAcmgAwIBAgIUfAbsrTrtK5a3TM5JA7a0QRjp9AwwCgYIKoZIzj0EAwIw
              QzEaMBgGA1UEAwwRaW5jdXMuZXhhbXBsZS5jb20xJTAjBgkqhkiG9w0BCQEWFmhv
              c3RtYXN0ZXJAZXhhbXBsZS5jb20wIBcNMjQwOTA1MDI1MjQ2WhgPMjEyNDA4MTIw
              MjUyNDZaMEMxGjAYBgNVBAMMEWluY3VzLmV4YW1wbGUuY29tMSUwIwYJKoZIhvcN
              AQkBFhZob3N0bWFzdGVyQGV4YW1wbGUuY29tMHYwEAYHKoZIzj0CAQYFK4EEACID
              YgAEqHAZUXsZu8C45RFsRDlyuUXj3Qe+zfszIKbqCrAtntDaw+TzWHwZUB2ndhiP
              xBrqeU4RT4og3wIA7+L7gTYsY8Pyv3mQIe8NA4UPTgs/AQHcyVg5ndsr3WkwFHjk
              wTcKo3wwejAMBgNVHRMBAf8EAjAAMA4GA1UdDwEB/wQEAwIFoDAdBgNVHSUEFjAU
              BggrBgEFBQcDAgYIKwYBBQUHAwEwHAYDVR0RBBUwE4IRaW5jdXMuZXhhbXBsZS5j
              b20wHQYDVR0OBBYEFAdU1B+55KHpxq6oqPDWYM54tgkaMAoGCCqGSM49BAMCA2cA
              MGQCMCjSchJqhiY3o/rc6AQDyTUz0RU1lz62ojqIupl6dq6mx67CZsvEbLxx6xKa
              afysxwIwBSIMECbait2CgcDm+WRGs989gexe/kLrJbbzp8HlEdR7ImMUSFWXur8k
              DJOVYN23
              -----END CERTIFICATE-----
            '');
            tls-key = builtins.toString (pkgs.writers.writeText "snakeoil-key.pem"
            ''
              -----BEGIN PRIVATE KEY-----
              MIG2AgEAMBAGByqGSM49AgEGBSuBBAAiBIGeMIGbAgEBBDAD8vRWtf399bYUmNGB
              6kt8bGxfNON+U87bhIaTrYex36fQrCyhLMZJ8MYbTNyVARChZANiAASocBlRexm7
              wLjlEWxEOXK5RePdB77N+zMgpuoKsC2e0NrD5PNYfBlQHad2GI/EGup5ThFPiiDf
              AgDv4vuBNixjw/K/eZAh7w0DhQ9OCz8BAdzJWDmd2yvdaTAUeOTBNwo=
              -----END PRIVATE KEY-----
            '');
          };
        };
        powerdns =
        {
          enable = true;
          extraConfig =
          ''
            api=no
            remote-connection-string=pipe:command=${pkgs.writeShellScript "lxddns-http-pipe" "${pkgs.lxddns-http}/bin/lxddns-http pipe --loglevel info,lxddns=trace --domain incus.example.com. --hostmaster hostmaster.example.com --remote https://incus.example.com:${builtins.toString config.services.lxddns-responder.http.listenPort} --soa-ttl 64 --aaaa-ttl 256"},timeout=5000
            launch=remote
            negquery-cache-ttl=1
            local-address=::
            # needed since 4.5
            zone-cache-refresh-interval=0
          '';
        };
      };
      environment.systemPackages = with pkgs; [ curl dig jq ];
      virtualisation =
      {
        emptyDiskImages = [ 10240 ];
        incus =
        {
          enable = true;
          socketActivation = false;
          softDaemonRestart = false;
        };
      };
      networking =
      {
        extraHosts = "::1 incus.example.com";
        nftables.enable = true;
        useDHCP = false;
      };
      systemd.network =
      {
        enable = true;
        wait-online.extraArgs = [ "--interface=br0" ];
        networks =
        {
          "50-internal" =
          {
            enable = true;
            name = "br0";
            addresses = [ { Address = "2001:db8::1/64"; DuplicateAddressDetection = "none"; } ];
            networkConfig =
            {
              IPv6Forwarding = true;
              IPv6SendRA = true;
              IPv6AcceptRA = false;
              ConfigureWithoutCarrier = true;
            };
            ipv6SendRAConfig =
            {
              EmitDomains = true;
              Domains = [ "incus.example.com" ];
            };
            ipv6Prefixes =
            [
              {
                Prefix = "2001:db8::/64";
              }
            ];
            linkConfig =
            {
              RequiredFamilyForOnline = "ipv6";
              RequiredForOnline = "no-carrier";
            };
          };
        };
        netdevs =
        {
          "50-internal" =
          {
            netdevConfig =
            {
              Name = "br0";
              Kind = "bridge";
            };
          };
        };
      };
    };
  };

  testScript =
    let
      preseed = pkgs.writers.writeText "incus-preseed.yaml"
      ''
        config:
          images.auto_update_interval: "0"
        networks: []
        storage_pools:
        - config:
            source: /dev/vdb
          description: ""
          name: default
          driver: btrfs
        profiles:
        - config: {}
          description: ""
          devices:
            eth0:
              name: eth0
              nictype: bridged
              parent: br0
              type: nic
            root:
              path: /
              pool: default
              type: disk
          name: default
        projects: []
        cluster: null
      '';
      guestConfig = { modulesPath, ... }:
      {
        imports = [ (modulesPath + "/virtualisation/lxc-container.nix") ];
        config =
        {
          nixpkgs.hostPlatform = pkgs.system;
        };
      };
      guest = pkgs.nixos guestConfig;
      image = guest.config.system.build.tarball;
      metadata = guest.config.system.build.metadata;
    in
      ''
        start_all()
        host.wait_for_unit("default.target")

        host.succeed("incus admin init --preseed < ${preseed}")
        host.succeed("incus ls >&2")
        host.succeed("incus image import --alias nixos ${metadata}/tarball/${guest.config.image.fileName} ${image}/tarball/${guest.config.image.fileName}")
        host.succeed("incus launch -e nixos guest")
        host.succeed("incus ls >&2")
        host.wait_until_succeeds("incus ls --format csv --columns 6 | grep ^2001:db8:")
        host.succeed("incus ls >&2")
        host.succeed("curl -s https://incus.example.com:9132/resolve/v1/guest | jq --exit-status '.V1.AnyMatch[0]? // \"\" | test(\"\\\\A2001:db8:\")'")
        host.succeed("dig +short guest.incus.example.com @::1 AAAA | grep ^2001:db8:")
      '';
}
