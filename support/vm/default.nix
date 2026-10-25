# The Dojo VM: an arm64 Linux kernel and an initramfs that boot under QEMU with
# Apple's hypervisor on an Apple silicon Mac. Every Linux path here comes from
# the binary cache, so building it needs no Linux machine.
{
  nixpkgs ? fetchTarball {
    # nixos-26.05
    url = "https://github.com/NixOS/nixpkgs/archive/1f607aa79b6356ed45d3cd3c8079c196e79efa2c.tar.gz";
    sha256 = "1s7mrwxqalq59bqklyi3015kr2sy1kvf8a8s6d9ahchv7d3fn39j";
  },
}:
let
  host = import nixpkgs { system = "aarch64-darwin"; };
  guest = import nixpkgs { system = "aarch64-linux"; };
  kernel = guest.linux;
  busybox = guest.pkgsStatic.busybox;
in
{
  inherit (host) qemu coreutils;
  inherit kernel;

  # nixpkgs' Linux builder: a NixOS VM that builds Dojo's binaries for arm64.
  builder = host.darwin.linux-builder.nixosConfig.system.build.vm;

  initrd =
    host.runCommand "dojo-initrd"
      {
        nativeBuildInputs = [
          host.cpio
          host.gzip
        ];
      }
      ''
        mkdir -p root/bin root/root root/dev root/proc root/sys root/run root/tmp
        cp ${busybox}/bin/busybox root/bin/busybox
        for applet in $(${host.coreutils}/bin/cat ${./applets}); do
          ln -s busybox "root/bin/$applet"
        done
        cp ${./init} root/init
        cp ${./check} root/bin/check
        cp ${./lease} root/bin/lease
        chmod 0755 root/init root/bin/check root/bin/lease
        mkdir -p root/etc
        echo "root:x:0:0:root:/root:/bin/sh" > root/etc/passwd
        echo "root:x:0:" > root/etc/group
        (cd root && find . | cpio --quiet -o -H newc -R 0:0) | gzip -9 > $out
      '';
}
