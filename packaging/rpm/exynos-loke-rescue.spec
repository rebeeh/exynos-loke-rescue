Name:           exynos-loke-rescue
Version:        1.0.0
Release:        1%{?dist}
Summary:        Deterministic Linux firmware recovery engine for Samsung Exynos SoCs

License:        MIT
URL:            https://github.com/rebeeh/exynos-loke-rescue
Source0:        %{name}-%{version}.tar.gz

BuildRequires:  rust >= 1.70
BuildRequires:  cargo
Requires:       systemd-udev

%description
Exynos LOKE Rescue is a clean-room, memory-safe Linux-native firmware flashing
engine and deterministic partition recovery tool for Samsung Exynos SoCs. It
interfaces directly with Odin/LOKE USB endpoints (04e8:685d), bypassing
Heimdall protocol hangs and samloader-rs dynamic super partition crashes.

%prep
%autosetup

%build
cargo build --release

%install
rm -rf $RPM_BUILD_ROOT
install -D -p -m 0755 target/release/%{name} %{buildroot}%{_bindir}/%{name}
install -D -p -m 0644 udev/60-exynos-loke-rescue.rules %{buildroot}%{_udevrulesdir}/60-exynos-loke-rescue.rules
install -D -p -m 0644 target/man/%{name}.1 %{buildroot}%{_mandir}/man1/%{name}.1
install -D -p -m 0644 target/completions/%{name}.bash %{buildroot}%{_datadir}/bash-completion/completions/%{name}
install -D -p -m 0644 target/completions/_%{name} %{buildroot}%{_datadir}/zsh/site-functions/_%{name}
install -D -p -m 0644 target/completions/%{name}.fish %{buildroot}%{_datadir}/fish/vendor_completions.d/%{name}.fish

%post
udevadm control --reload-rules 2>/dev/null || :
udevadm trigger --subsystem-match=usb 2>/dev/null || :

%postun
if [ $1 -eq 0 ]; then
    udevadm control --reload-rules 2>/dev/null || :
fi

%files
%license LICENSE
%doc README.md PROTOCOL.md
%{_bindir}/%{name}
%{_udevrulesdir}/60-exynos-loke-rescue.rules
%{_mandir}/man1/%{name}.1*
%{_datadir}/bash-completion/completions/%{name}
%{_datadir}/zsh/site-functions/_%{name}
%{_datadir}/fish/vendor_completions.d/%{name}.fish

%changelog
* Sat Nov 14 2026 Rabeeh Fouz Ahammad VK <rabeeh@users.noreply.github.com> - 1.0.0-1
- Initial production release with native nusb Odin v3/v4 wire protocol engine.
