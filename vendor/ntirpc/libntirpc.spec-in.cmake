
%undefine		_hardened_build

# Conditionally enable some options, disable others.
#
# 1. rpmbuild accepts these options (gpfs as example):
#    --with gpfs
#    --without gpfs

%define on_off_switch() %%{?with_%1:ON}%%{!?with_%1:OFF}

# A few explanation about % bcond_with and % bcond_without
# /!\ be careful: this syntax can be quite messy
# % bcond_with means you add a "--with" option, default = without this feature
# % bcond_without adds a"--without" so the feature is enabled by default

@BCOND_MONITORING@ monitoring
%global use_monitoring %{on_off_switch monitoring}

Name:		libntirpc
Version:	@NTIRPC_VERSION@
Release:	1%{?dev:%{dev}}%{?dist}
Summary:	New Transport Independent RPC Library
Group:		System Environment/Libraries
License:	BSD
Url:		https://github.com/nfs-ganesha/ntirpc

Source0:	https://github.com/nfs-ganesha/ntirpc/archive/v%{version}/ntirpc-%{version}.tar.gz

BuildRequires:	cmake
BuildRequires:	krb5-devel
# libtirpc has /etc/netconfig, most machines probably have it anyway
# for NFS client
Requires:	libtirpc

%description
This package contains a new implementation of the original libtirpc,
transport-independent RPC (TI-RPC) library for NFS-Ganesha. It has
the following features not found in libtirpc:
 1. Bi-directional operation
 2. Full-duplex operation on the TCP (vc) transport
 3. Thread-safe operating modes
 3.1 new locking primitives and lock callouts (interface change)
 3.2 stateless send/recv on the TCP transport (interface change)
 4. Flexible server integration support
 5. Event channels (remove static arrays of xprt handles, new EPOLL/KEVENT
    integration)

%package devel
Summary:	Development headers for %{name}
Requires:	%{name}%{?_isa} = %{version}

%description devel
Development headers and auxiliary files for developing with %{name}.

%prep
%setup -q -n ntirpc-%{version}

%build
%cmake . -DOVERRIDE_INSTALL_PREFIX=/usr -DTIRPC_EPOLL=1 -DUSE_GSS=ON -DUSE_MONITORING=%{use_monitoring} "-GUnix Makefiles"

%cmake_build %{?_smp_mflags}

%install
mkdir -p %{buildroot}%{_libdir}/pkgconfig

%cmake_install

ln -s %{name}.so.%{version} %{buildroot}%{_libdir}/%{name}.so.4

%post -p /sbin/ldconfig

%postun -p /sbin/ldconfig

%files
%{_libdir}/libntirpc.so.*
%if %{with monitoring}
%{_libdir}/libntirpcmonitoring.so.*
%endif
%{!?_licensedir:%global license %%doc}
%license COPYING
%doc NEWS README

%files devel
%{_libdir}/libntirpc.so
%if %{with monitoring}
%{_libdir}/libntirpcmonitoring.so
%endif
%dir %{_includedir}/ntirpc
%{_includedir}/ntirpc/*
%{_libdir}/pkgconfig/libntirpc.pc

%changelog
* Wed Jul 19 2017 Daniel Gryniewicz <dang at redhat.com> 1.6.0-1
- Upstream spec file
