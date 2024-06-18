{ config, pkgs, lib, ... }:
  let
    defaultBinary = { amqp = "lxddns-amqp"; http = "lxddns-http"; none = null; };
    defaultPackage = { amqp = "lxddns-amqp"; http = "lxddns-http"; none = "lxddns"; };
    defaultArgs =
    {
      none = null;
      # not implemented
      amqp = null;
      http = [ "responder" "-v" "info" "--tls-chain" "/run/credentials/lxddns-responder.service/cert.pem" "--tls-key" "/run/credentials/lxddns-responder.service/key.pem" "--https-bind" "${cfg.http.listenAddress}:${toString cfg.http.listenPort}" ];
    };
    cfg = config.services.lxddns-responder;
  in
    {
      options =
      {
        services.lxddns-responder =
        {
          enable = lib.mkEnableOption (lib.mdDoc "lxddns-responder");
          package = lib.mkPackageOptionMD pkgs "lxddns"
          {
            default = [ defaultPackage.${cfg.protocol} ];
          };
          protocol = lib.mkOption
          {
            default = "http";
            type = lib.types.enum [ "http" "amqp" "none" ];
            description =
            ''
              Which protocol variant to use.
              This option determines defaults for the binary used and the CLI parameters deployed.
              Use `none` to disable defaults.
            '';
          };
          user = lib.mkOption
          {
            default = "lxddns";
            type = lib.types.str;
            description =
            ''
              User to run the service as.

              ::: {.note}
              If set to default the user will be automatically created, otherwise you are responsible for providing the user configuration.
              :::
            '';
          };
          group = lib.mkOption
          {
            default = "lxddns";
            type = lib.types.str;
            description =
            ''
              Group to run the service as.

              ::: {.note}
              If set to default the group will be automatically created, otherwise you are responsible for providing the group configuration.
              :::
            '';
          };
          sudo = lib.mkOption
          {
            default = true;
            type = lib.types.bool;
            description =
            ''
              Enable default sudo access to query the LXD service.

              ::: {.note}
              The sudo queries used by the service are hardcoded in the Rust code, however it is possible to deny queries to specific hosts, in theory.
              :::
            '';
          };
          silentSudo = lib.mkOption
          {
            default = cfg.sudo;
            type = lib.types.bool;
            description =
            ''
              Additional sudo configuration to not log the LXD queries to syslog.
              May improve performance through disk IO reduction.
            '';
          };
          virt-command = lib.mkOption
          {
            default = "${pkgs.lxd}/bin/lxc";
            defaultText = lib.literalExpression "\${pkgs.lxd}/bin/lxc";
            type = lib.types.str;
            description =
            ''
              Command used passed to `--command` option of *lxddns*.
              This ensures compatibility with differing software such as Incus and LXD.
            '';
          };
          binary = lib.mkOption
          {
            default = defaultBinary.${cfg.protocol};
            type = lib.types.str;
            description =
            ''
              Name of the binary to use from the package.
              If diverging protocols (or custom packages) this may need adjustments.
              Prefer setting `protocol` if possible.
            '';
          };
          args = lib.mkOption
          {
            default = defaultArgs.${cfg.protocol};
            type = lib.types.listOf lib.types.str;
            description =
            ''
              Arguments used in systemd service.
              This is mainly useful if you chose `none` as the `protocol`.
              Use `extraArgs` otherwise.
            '';
          };
          dependentArgs = lib.mkOption
          {
            default = [ "--command" cfg.virt-command ];
            defaultText = lib.literalExpression ''[ "--command" cfg.virt-command ]'';
            type = lib.types.listOf lib.types.str;
            description =
            ''
              Dependent arguments passed to the systemd service conditionally.
            '';
          };
          extraArgs = lib.mkOption
          {
            default = [];
            type = lib.types.listOf lib.types.str;
            description =
            ''
              Additional arguments passed to the systemd service.
              Can be used for loglevel adjustments or similar.
            '';
          };
          http =
          {
            listenAddress = lib.mkOption
            {
              default = "[::]";
              type = lib.types.str;
              description =
              ''
                Address to bind lxddns to.
              '';
            };
            listenPort = lib.mkOption
            {
              default = 9132;
              type = lib.types.int;
              description =
              ''
                Port to bind lxddns to.
              '';
            };
            tls-cert = lib.mkOption
            {
              default = "/var/lib/acme/${config.networking.fqdnOrHostName}/fullchain.pem";
              defaultText = "`/var/lib/acme/\${config.networking.fqdnOrHostName}/fullchain.pem`";
              type = lib.types.str;
              description =
              ''
                TLS certificate to use for TLS.
                
                ::: {.note}
                It is the users responsibility to acquire this certificate and to trigger a restart of this service whenever the certificate changes.
                :::
              '';
            };
            tls-key = lib.mkOption
            {
              default = "/var/lib/acme/${config.networking.fqdnOrHostName}/key.pem";
              defaultText = "`/var/lib/acme/\${config.networking.fqdnOrHostName}/key.pem`";
              type = lib.types.str;
              description =
              ''
                TLS key to use for TLS.
                
                ::: {.note}
                It is the users responsibility to acquire this key and to trigger a restart of this service whenever the key changes.
                :::
              '';
            };
          };
        };
      };

      config = lib.mkMerge
      [
        (lib.mkIf cfg.enable
        {
          systemd.services =
          {
            lxddns-responder =
            {
              enable = true;
              description = "lxddns responder";
              # requires sudo and lxd
              path = [ "/run/wrappers" ];
              unitConfig =
              {
                Type = "simple";
              };
              serviceConfig =
              {
                ExecStart = "${cfg.package}/bin/${cfg.binary} ${toString cfg.args} ${toString cfg.dependentArgs} ${toString cfg.extraArgs}";
                User = cfg.user;
                Group = cfg.group;
                LoadCredential =
                [
                  "cert.pem:${cfg.http.tls-cert}"
                  "key.pem:${cfg.http.tls-key}"
                ];
              };
              wantedBy = [ "multi-user.target" ];
            };
          };
        })
        (lib.mkIf (cfg.enable && cfg.sudo)
        {
          security.sudo =
          {
            enable = true;
            extraRules = lib.mkOrder 1500
            [
              {
                users = [ cfg.user ];
                commands =
                [
                  { command = "${cfg.virt-command} query -- *"; options = [ "NOPASSWD" ]; }
                ];
              }
            ];
          };
        })
        (lib.mkIf (cfg.enable && cfg.silentSudo)
        {
          security.sudo.extraConfig =
          ''
            Defaults:${cfg.user} !syslog
          '';
        })
        (lib.mkIf (cfg.enable && cfg.user == "lxddns")
        {
          users.users.${cfg.user} =
          {
            isSystemUser = true;
            group = cfg.group;
          };
        })
        (lib.mkIf (cfg.enable && cfg.group == "lxddns")
        {
          users.groups.${cfg.group} = {};
        })
      ];
    }
