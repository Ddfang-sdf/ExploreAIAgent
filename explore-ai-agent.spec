Name:           explore-ai-agent
Version:        0.1.0
Release:        1%{?dist}
Summary:        AI-driven codebase exploration agent

License:        MIT
URL:            https://github.com/Ddfang-sdf/ExploreAIAgent
Source0:        %{name}-%{version}.tar.gz

BuildRequires:  rust
BuildRequires:  cargo
BuildRequires:  gcc

Requires:       glibc >= 2.17

%description
Explore AI Agent is an LLM-driven codebase exploration agent that
autonomously analyzes local codebases through a multi-agent pipeline:
fast keyword search -> quality evaluation -> optional deep exploration
-> final answer generation. All operations are read-only.

%prep
%setup -q

%build
cargo build --release

%install
mkdir -p %{buildroot}%{_bindir}
install -m 755 target/release/%{name} %{buildroot}%{_bindir}/%{name}

mkdir -p %{buildroot}%{_sysconfdir}/%{name}
install -m 644 config.template.yaml %{buildroot}%{_sysconfdir}/%{name}/config.yaml

mkdir -p %{buildroot}%{_sharedstatedir}/%{name}/workspace

mkdir -p %{buildroot}%{_unitdir}
cat > %{buildroot}%{_unitdir}/%{name}.service << 'SERVICE'
[Unit]
Description=Explore AI Agent
After=network-online.target
Wants=network-online.target

[Service]
Type=simple
ExecStart=%{_bindir}/%{name}
Environment=EXPLORE_CONFIG_PATH=%{_sysconfdir}/%{name}/config.yaml
Environment=EXPLORE_WORKSPACE__PATH=%{_sharedstatedir}/%{name}/workspace
WorkingDirectory=%{_sharedstatedir}/%{name}
StandardInput=tty
StandardOutput=journal
StandardError=journal
Restart=on-failure
User=nobody
Group=nobody

[Install]
WantedBy=multi-user.target
SERVICE

%files
%{_bindir}/%{name}
%dir %{_sysconfdir}/%{name}
%config(noreplace) %{_sysconfdir}/%{name}/config.yaml
%dir %{_sharedstatedir}/%{name}
%dir %{_sharedstatedir}/%{name}/workspace
%{_unitdir}/%{name}.service

%post
echo "Explore AI Agent installed."
echo "Edit %{_sysconfdir}/%{name}/config.yaml to configure your LLM API key."
echo "Start with: systemctl start %{name}"

%preun
if [ $1 -eq 0 ]; then
    systemctl stop %{name} 2>/dev/null || true
    systemctl disable %{name} 2>/dev/null || true
fi

%changelog
* Thu May 07 2026 Explore AI Agent Team - 0.1.0-1
- Initial RPM package
