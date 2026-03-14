# SPDX-License-Identifier: GPL-2.0

KDIR ?= /lib/modules/`uname -r`/build
KOUT ?= $(abspath $(CURDIR)/../../target/kernel)

default:
	mkdir -p $(KOUT)
	$(MAKE) -C $(KDIR) M=$$PWD MO=$(KOUT)

modules_install: default
	$(MAKE) -C $(KDIR) M=$$PWD MO=$(KOUT) modules_install

clean:
	$(MAKE) -C $(KDIR) M=$$PWD MO=$(KOUT) clean
