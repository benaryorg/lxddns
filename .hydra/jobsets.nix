{ refs, projectName, ... }:
  let
      isMainBranch = branch: branch == "main";
      jobForBranch = { name, ...}:
      {
        enabled = 1;
        hidden = false;
        description = "branch ${name}";
        checkinterval = if isMainBranch name then 60 else 15;
        schedulingshares = if isMainBranch name then 100 else 1;
        enableemail = false;
        emailoverride = "";
        keepnr =  if isMainBranch name then 16 else 4;
        type = 1;
        flake = "git+https://git.shell.bsocat.net/lxddns?ref=${name}";
      };
      parseRef = ref: let
          parts = builtins.match "([^\t]+)\t(.*)" ref;
          commit = builtins.head parts;
          rawref = builtins.head (builtins.tail parts);
          splitref = builtins.filter builtins.isString (builtins.split "/" rawref);
          len = builtins.length splitref;
        in
          if len != 3 || (builtins.head splitref) != "refs" then null else { type = builtins.elemAt splitref 1; name = builtins.elemAt splitref 2; commit = commit; };
      parseRefs = refs: let
          lines = builtins.filter builtins.isString (builtins.split "\n" refs);
          nonEmptyLines = builtins.filter (s: s != "") lines;
          parsedRefs = builtins.filter (x: x != null) (builtins.map parseRef nonEmptyLines);
          branches = builtins.filter ({ type, ...}: type == "heads") parsedRefs;
        in
          branches;
      parseRefFile = file: parseRefs (builtins.readFile file);
      jobspec = refs: let
          branches = parseRefFile refs;
          jobs = builtins.map ({ name, ... }@branch: { name = name; value = jobForBranch branch; }) branches;
        in
          builtins.listToAttrs jobs;
    in
      {
        jobsets =
            (builtins.derivation
            {
              system = builtins.currentSystem;
              name = "${projectName}-jobspec";
              builder = "/bin/sh";
              args =
              [
                (builtins.toFile "generate-jobspec.sh"
                ''
                  read -r jobspec <<"END"
                  ${builtins.toJSON (jobspec refs)}
                  END
                  printf "%s\\n" "$jobspec" > $out
                '')
              ];
            })
          //
            {
              meta =
              {
                description = "hydra jobset generation";
                license = [ { shortName = "AGPL-3.0-or-later"; } ];
                homepage = "https://git.shell.bsocat.net/lxddns/";
                maintainers = [ { email = "root@benary.org"; } ];
              };
            }
        ;
      }
