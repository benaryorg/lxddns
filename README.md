# What does it do?

*lxddns* links your [*LXD*](https://linuxcontainers.org/lxd/) internal databases across several servers using [*rabbitmq*](https://www.rabbitmq.com/) to provide public DNS for all your instances via IPv6.

It provides `AAAA` records under a domain as well as `NS` records pointing to those same `AAAA` records under the `_acme-challenge` subdomain so you can perform *dns01* ACME validation using a local DNS server.

Paired with certificates (e.g. via Let's Encrypt) including client-certificate validation this allows you to operate your services via regular public IPv6.
Effectively it also means you can migrate all your containers in between LXD nodes as they get new IPs from the new subnet of the host (or however you handle that) and will almost instantly show up using the new address (almost due to the TTL).

# How to use this?

The basic [rationale](#rationale) behind this is found further down in this document, this here is a more technical description of required components.

First off you need one or more nodes running [*LXD*](https://linuxcontainers.org/lxd/).

All of those nodes may assign IPv6 public addresses to their containers in whichever way you see fit.
I personally created a bridge device on which *radvd* announces the prefix, and *ndppd* on the external interface which *static*-ally responds to the entire `/64` (not needed for some hosters like *netcup*, needed for others like *Kimsufi*, this depends on how the net is routed).

Now you need a *sudoers* entry for the user under which *lxddns* will be running.
Why you ask?
Mostly because both `pdns ALL=(ALL) NOPASSWD: /usr/bin/lxc query -- *` and client-certificate based authenticate allow essentially the same access rights but the former was easier to implement (feel free to submit a PR for this).

*lxddns* can now access LXD, what's further needed is a [*rabbitmq*](https://www.rabbitmq.com/) cluster, the address of which can be provided as an environment variable.
This doesn't actually need to be a cluster but I'd highly recommend it.
My cluster is running spread across all host-nodes and each *lxddns* just asks *localhost*.

Now grab an authorative [*PowerDNS*](https://www.powerdns.com/) server and configure it to ask the Unix Domain Socket provided by *lxddns* (`remote-connection-string=unix:path=[…],timeout=5000` and `launch=remote`).

Start *lxddns* with appropriate arguments (see `--help`).

Set an NS record with any subset of your servers for the corresponding domain to delegate the domain (or subdomain) to the *lxddns* "network".

# Rationale

If you're running IPv6-only infrastructure you'll encounter one thing rather quick: you really do need DNS, and dnsmasq does not support its DHCP/DNS combo in IPv6 as it does in IPv4.
The basic idea is to have every host node equipped with its own IPv6 `/64` subnet and use this subnet for *SLAAC*.
Since my containers still do need to find one another and setting up automated DynDNS for all those hosts is kinda complicated and needs authorization I thought I could (ab-) use LXD, which I am using for containers, to resolve the names to IPs.
This technically works, however LXD Cluster is built for same-rack or same-DC clusters and doesn't work very well with latency.
So this is a working attempt at bringing dynamic DNS records derived from LXD to life.

# Future?

I will expand this program for my needs which are not very thorough, however I welcome pull requests *and* filed issues alike.
Currently there is no support for IPv4 although it would be a semi-trivial change (I just can't test the change).
TTL and similar cannot be adjusted via command line parameters at the moment.
There is no configuration file support (and likely will never be any).

