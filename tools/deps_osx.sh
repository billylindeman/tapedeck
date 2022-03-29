#!/bin/bash


HOMEBREW_NO_AUTO_UPDATE=1 brew install \
    autoconf \
    automake \
    gettext \
    pkg-config \
    docbook-xsl

mkdir -p /tmp/build-tapedeck-osx
cd /tmp/build-tapedeck-osx


if [ -f "xorg-server" ]; then
    git clone --depth 1 git@github.com:XQuartz/xorg-server.git
fi
cd xorg-server

export ACLOCAL="aclocal -I /opt/X11/share/aclocal -I /usr/local/share/aclocal"
export PKG_CONFIG_PATH="/opt/X11/share/pkgconfig:/opt/X11/lib/pkgconfig"
export CFLAGS="-Wall -O0 -ggdb3 -arch i386 -arch x86_64 "
export OBJCFLAGS=$CFLAGS
export LDFLAGS=$CFLAGS
export CC="gcc-11" 
export CXX="g++-11" 
export XML_CATALOG_FILES=$(brew --prefix)/etc/xml/catalog


#autoreconf -fvi
./configure \
    --prefix=/usr/local/tapedeck/xvfb \
    --disable-dependency-tracking \
    --enable-maintainer-mode \
    --disable-xquartz \
    --enable-xvfb 
#    --enable-xnest 
#    --enable-kdrive




make -j$(sysctl -n machdep.cpu.threadcount)
#sudo make install

