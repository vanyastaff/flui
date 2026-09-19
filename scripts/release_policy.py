#!/usr/bin/env python3
"""Check release roles and Cargo's retained declaration graph; never publish."""
import argparse
from collections import deque
from dataclasses import dataclass, field
import json
from pathlib import Path
import subprocess
import sys
import tarfile
import tomllib

ROLES = {'facade', 'cli', 'support', 'private'}
REGISTRY_INDEX = 'https://github.com/rust-lang/crates.io-index'


@dataclass
class Report:
    errors: list[str] = field(default_factory=list)
    selected: list[str] = field(default_factory=list)
    reasons: dict[str, list[str]] = field(default_factory=dict)
    private: list[str] = field(default_factory=list)
    dependency_cycles: list[dict] = field(default_factory=list)
    checkout_only_dev: list[dict] = field(default_factory=list)


def manifests(root):
    """Only repository package roots, excluding build products and nested checkouts."""
    seen = {root / 'Cargo.toml'}
    yield root / 'Cargo.toml'
    workspace = tomllib.loads((root / 'Cargo.toml').read_text()).get('workspace', {})
    for member in workspace.get('members', []):
        for path in sorted(root.glob(member + '/Cargo.toml')):
            if path not in seen:
                seen.add(path)
                yield path
    for directory in ('crates', 'examples', 'tools'):
        for path in sorted((root / directory).rglob('Cargo.toml')):
            if path not in seen and not {'target', '.git', '.claude'}.intersection(path.relative_to(root).parts):
                seen.add(path)
                yield path


def inherited(value, key, workspace):
    if isinstance(value, dict) and value.get('workspace') is True:
        return workspace.get('package', {}).get(key)
    return value


def declarations(document, directory, root, workspace, *, retained_only=True):
    """Yield declarations after workspace inheritance, preserving version presence."""
    scopes = [(None, document)] + list(document.get('target', {}).items())
    for target, scope in scopes:
        for table, kind in [('dependencies', 'normal'), ('build-dependencies', 'build'),
                            ('dev-dependencies', 'dev')]:
            for alias, original in scope.get(table, {}).items():
                spec = {'version': original} if isinstance(original, str) else dict(original)
                origin = directory
                if spec.get('workspace') is True:
                    base = workspace.get('dependencies', {}).get(alias)
                    if base is None:
                        raise ValueError(f'unknown inherited dependency {alias}')
                    local = spec
                    spec = {'version': base} if isinstance(base, str) else dict(base)
                    spec['features'] = sorted(set(spec.get('features', [])) | set(local.get('features', [])))
                    if 'optional' in local:
                        spec['optional'] = local['optional']
                    if 'default-features' in local:
                        if local['default-features'] is False and spec.get('default-features', True):
                            raise ValueError(f'{alias}: default-features=false cannot disable workspace defaults in edition 2024')
                        spec['default-features'] = local['default-features']
                    origin = root
                # Cargo drops only dev declarations WITHOUT a version key.
                if retained_only and kind == 'dev' and 'version' not in spec:
                    continue
                yield alias, spec, origin, kind, target


def check(root):
    root = Path(root).resolve()
    report = Report()
    root_document = tomllib.loads((root / 'Cargo.toml').read_text())
    workspace = root_document.get('workspace', {})
    policy = tomllib.loads((root / 'docs/workspace-layers.toml').read_text())
    roles = {}
    for member in policy.get('member', []):
        name, role = member['name'], member.get('release_role')
        if name in roles:
            report.errors.append(f'duplicate release classification: {name}')
        roles[name] = role
        if role not in ROLES:
            report.errors.append(f'{name}: missing or unknown release_role {role!r}')

    packages = {}
    directories = {}
    for path in manifests(root):
        document = tomllib.loads(path.read_text())
        package = document.get('package')
        if not package:
            continue
        name = package['name']
        if name in packages:
            report.errors.append(f'duplicate package identity: {name}')
            continue
        directory = path.parent.resolve()
        packages[name] = (document, directory)
        directories[directory] = name
        publish = inherited(package.get('publish'), 'publish', workspace)
        role = roles.get(name)
        if role is None and publish is False:
            role = 'private'
            roles[name] = role
        if role is None:
            report.errors.append(f'{name}: unclassified publishable package at {path.relative_to(root)}')
        elif role == 'private':
            report.private.append(name)
            if publish is not False:
                report.errors.append(f'{name}: private package must set publish = false')
        elif role in ROLES and publish != ['crates-io']:
            report.errors.append(f'{name}: {role} must restrict publish to ["crates-io"]')

    for name in roles.keys() - packages.keys():
        report.errors.append(f'{name}: release classification has no package')
    for role, expected in [('facade', 'flui'), ('cli', 'flui-cli')]:
        actual = sorted(name for name, value in roles.items() if value == role)
        if actual != [expected]:
            report.errors.append(f'{role} product must be exactly {expected}; found {actual}')
    if 'flui' in packages:
        doc, directory = packages['flui']
        lib = doc.get('lib', {})
        if not (directory / lib.get('path', 'src/lib.rs')).is_file():
            report.errors.append('flui: facade must have a library target')
    if 'flui-cli' in packages:
        doc, directory = packages['flui-cli']
        if not any(binary.get('name') == 'flui' for binary in doc.get('bin', [])):
            report.errors.append('flui-cli: CLI product must declare its flui binary target')

    record_fields = ('source', 'package', 'alias', 'target', 'path')
    checkout_records = {}
    for record in policy.get('checkout_only_dev', []):
        if any(not isinstance(record.get(key), str) or not record[key] for key in (*record_fields, 'why')):
            report.errors.append(f'invalid checkout-only dev record: {record}')
            continue
        key = tuple(record[field] for field in record_fields)
        if key in checkout_records:
            report.errors.append(f'duplicate checkout-only dev record: {key}')
        checkout_records[key] = record
    observed_checkout = set()

    edges = {name: set() for name in packages}
    build_edges = {name: set() for name in packages}
    edge_details = []
    for name, (document, directory) in packages.items():
        if roles.get(name) not in {'facade', 'cli', 'support'}:
            continue
        try:
            dependencies = list(declarations(document, directory, root, workspace, retained_only=False))
        except ValueError as error:
            report.errors.append(f'{name}: {error}')
            continue
        for alias, spec, origin, kind, target in dependencies:
            label = f'{name} -> {alias} ({kind}, target={target or "all"})'
            dependency_name = spec.get('package', alias)
            requirement = spec.get('version')
            omitted = kind == 'dev' and 'version' not in spec
            if 'path' in spec:
                destination = (origin / spec['path']).resolve()
                found = directories.get(destination)
                if found is None:
                    report.errors.append(f'{label}: unknown internal dependency path {destination}')
                    continue
                if found != dependency_name:
                    report.errors.append(f'{label}: package/path mismatch: {dependency_name} versus {found}')
                    continue
            if omitted:
                if 'path' not in spec or 'git' in spec or spec.get('registry') not in (None, 'crates-io') or spec.get('registry-index') not in (None, REGISTRY_INDEX):
                    report.errors.append(f'{label}: checkout-only dev dependency must use a local package path')
                # Check raw declarations before Cargo's unversioned-dev omission.
                # Only an exact, reviewed local package edge may disappear.
                resolved_path = str(destination.relative_to(root)) if 'path' in spec else ''
                key = (name, dependency_name, alias, target or 'all', resolved_path)
                if key not in checkout_records:
                    report.errors.append(f'{label}: unrecorded checkout-only dev dependency {key}')
                else:
                    observed_checkout.add(key)
                    report.checkout_only_dev.append(checkout_records[key])
                continue
            if not isinstance(requirement, str) or not requirement.strip() or requirement.strip() == '*':
                report.errors.append(f'{label}: retained dependency requires a non-wildcard version')
            if spec.get('registry') not in (None, 'crates-io') or spec.get('registry-index') not in (None, REGISTRY_INDEX):
                report.errors.append(f'{label}: retained dependency must resolve from crates.io')
            if 'path' not in spec and dependency_name not in packages:
                if dependency_name.startswith('flui-') or dependency_name == 'flui':
                    report.errors.append(f'{label}: unknown internal package {dependency_name}')
                # git+version is a valid dual-location dependency: Cargo removes git.
                continue
            if dependency_name not in packages:
                continue
            edges[name].add(dependency_name)
            edge_details.append({'from': name, 'to': dependency_name, 'kind': kind, 'target': target})
            if kind != 'dev':
                build_edges[name].add(dependency_name)
            if roles.get(dependency_name) == 'private':
                report.errors.append(f'{label}: retained dependency reaches private package {dependency_name}')
            target_document, _ = packages[dependency_name]
            version = inherited(target_document['package'].get('version'), 'version', workspace)
            allowed = {f'={version}'} if '-' in str(version) else {version, f'^{version}', f'={version}'}
            if requirement not in allowed:
                report.errors.append(f'{label}: requirement {requirement!r} must match cohort version {version!r}; allowed {sorted(allowed)}')

    for key in sorted(checkout_records.keys() - observed_checkout):
        report.errors.append(f'stale checkout-only dev record: {key}')
    report.checkout_only_dev.sort(key=lambda record: tuple(record[field] for field in record_fields))

    queue = deque()
    for product in ('flui', 'flui-cli'):
        if product in packages:
            report.reasons[product] = [product]
            queue.append(product)
    while queue:
        source = queue.popleft()
        for destination in sorted(edges[source]):
            if destination not in report.reasons:
                report.reasons[destination] = report.reasons[source] + [destination]
                queue.append(destination)
    for name, role in roles.items():
        if role == 'support' and name not in report.reasons:
            report.errors.append(f'{name}: support package is unreachable from the products')
    # Dev cycles are legal and affect archive closure, not build order.
    visiting, visited = set(), set()
    def visit(name, path):
        if name in visiting:
            report.errors.append('normal/build dependency cycle: ' + ' -> '.join(path + [name]))
            return
        if name in visited:
            return
        visiting.add(name)
        for destination in sorted(build_edges[name]):
            visit(destination, path + [name])
        visiting.remove(name)
        visited.add(name)
    for name in sorted(packages):
        visit(name, [])
    report.selected = sorted(name for name in report.reasons if roles.get(name) in {'facade', 'cli', 'support'})
    # Report full retained SCCs, without confusing legal dev cycles with build cycles.
    index, low, stack, active = {}, {}, [], set()
    def component(name):
        index[name] = low[name] = len(index)
        stack.append(name)
        active.add(name)
        for destination in sorted(edges[name]):
            if destination not in report.selected:
                continue
            if destination not in index:
                component(destination)
                low[name] = min(low[name], low[destination])
            elif destination in active:
                low[name] = min(low[name], index[destination])
        if low[name] == index[name]:
            members = []
            while True:
                member = stack.pop()
                active.remove(member)
                members.append(member)
                if member == name:
                    break
            if len(members) > 1 or name in edges[name]:
                report.dependency_cycles.append({
                    'packages': sorted(members),
                    'edges': [edge for edge in edge_details if edge['from'] in members and edge['to'] in members],
                })
    for name in report.selected:
        if name not in index:
            component(name)
    report.private.sort()
    return report


def package_arguments(selected, preview_dirty=False):
    arguments = ['cargo', 'package', '--registry', 'crates-io', '--no-verify']
    if preview_dirty:
        arguments.append('--allow-dirty')
    for name in selected:
        arguments.extend(['-p', name])
    return arguments


def declaration_identity(alias, spec, kind, target):
    """Compare Cargo-normalized declarations including feature activation semantics."""
    return (kind, target, alias, spec.get('package', alias), spec.get('version'),
            tuple(sorted(set(spec.get('features', [])))), spec.get('optional', False),
            spec.get('default-features', True))


def verify_archives(root, selected):
    """Inspect Cargo's normalized manifests, not source manifests or --list output."""
    errors = []
    workspace = tomllib.loads((root / 'Cargo.toml').read_text()).get('workspace', {})
    documents, directories = {}, {}
    for path in manifests(root):
        document = tomllib.loads(path.read_text())
        name = document.get('package', {}).get('name')
        if name:
            documents[name] = document
            directories[name] = path.parent
    for name in selected:
        version = inherited(documents[name]['package'].get('version'), 'version', workspace)
        archive = root / 'target/release-policy/package' / f'{name}-{version}.crate'
        with tarfile.open(archive) as package:
            manifest = package.extractfile(f'{name}-{version}/Cargo.toml')
            if manifest is None:
                errors.append(f'{name}: normalized manifest absent')
                continue
            normalized = tomllib.loads(manifest.read().decode())
        expected = {declaration_identity(alias, spec, kind, target)
                    for alias, spec, _, kind, target in declarations(documents[name],
                        directories[name], root, workspace)}
        actual = set()
        for alias, spec, _, kind, target in declarations(normalized, root, root, {}, retained_only=False):
            actual.add(declaration_identity(alias, spec, kind, target))
            if 'path' in spec or 'git' in spec:
                errors.append(f'{name}: normalized {kind} dependency {alias} retains local source')
            package_name = spec.get('package', alias)
            if package_name in documents and package_name not in selected:
                errors.append(f'{name}: normalized dependency {package_name} outside selected release set')
        if actual != expected:
            errors.append(f'{name}: normalized retained declarations differ from the policy graph: missing={expected - actual}, extra={actual - expected}')
    return errors


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--root', type=Path, default=Path(__file__).resolve().parents[1])
    parser.add_argument('--json', action='store_true', help='print the computed inventory')
    parser.add_argument('--package', action='store_true', help='create and inspect local archives without building or publishing')
    parser.add_argument('--preview-dirty', action='store_true', help='explicitly permit dirty local archive previews')
    args = parser.parse_args()
    root = args.root.resolve()
    report = check(root)
    if args.preview_dirty and not args.package:
        parser.error('--preview-dirty requires --package')
    if report.errors:
        for error in report.errors:
            print(f'release-policy: {error}', file=sys.stderr)
        return 1
    if args.package:
        command = package_arguments(report.selected, args.preview_dirty)
        command.extend(['--target-dir', str(root / 'target/release-policy')])
        completed = subprocess.run(command, cwd=root)
        if completed.returncode:
            print('release-policy: Cargo archive creation failed; declaration closure is not proof of packageability. '
                  'Retained dev cycles can require versions already present in the registry; '
                  'inspect Cargo diagnostics and release-inventory dependency_cycles. No versions or edges were changed.', file=sys.stderr)
            return completed.returncode
        report.errors.extend(verify_archives(root, report.selected))
        if report.errors:
            for error in report.errors:
                print(f'release-policy: {error}', file=sys.stderr)
            return 1
    if args.json:
        print(json.dumps({'packages': report.selected, 'private': report.private, 'reasons': report.reasons, 'dependency_cycles': report.dependency_cycles, 'checkout_only_dev': report.checkout_only_dev}, indent=2))
    else:
        print(f'release-policy: {len(report.selected)} product/support packages; {len(report.private)} private packages; closure checked')
    return 0


if __name__ == '__main__':
    sys.exit(main())
