import 'dart:convert';
import 'dart:io';

import 'package:code_assets/code_assets.dart';
import 'package:crypto/crypto.dart';
import 'package:hooks/hooks.dart';

const _libraryBaseName = 'chat_core';
const _defaultOwner = 'SecureGroupTP';
const _defaultRepo = 'chat_core';
const _defaultTag = 'v0.0.8';

void main(List<String> args) async {
  await build(args, (input, output) async {
    if (!input.config.buildCodeAssets) return;

    final target = _Target.from(input.config.code);
    final defines = _Defines(input);
    final release = await _Release.fetch(
      owner: defines.githubOwner,
      repo: defines.githubRepo,
      tag: defines.releaseTag,
    );

    final choice =
        target.chooseAsset(release, allowStatic: defines.allowStaticLinking) ??
        await target.tryBuildLocalDynamic(input);
    if (choice == null) {
      throw UnsupportedError(
        'No chat_core native asset for ${target.os.name}/${target.arch.name} '
        'in ${defines.githubOwner}/${defines.githubRepo} ${defines.releaseTag}. '
        'Create a release with dynamic libraries, or build from a local '
        'chat_core checkout with cargo available. '
        'Available assets: ${release.assetNames.join(', ')}',
      );
    }

    final outputFile = input.outputDirectoryShared.resolve(
      '${defines.releaseTag}/${target.key}/${choice.bundleName}',
    );
    if (choice.downloadUrl != null) {
      await _downloadIfNeeded(choice.downloadUrl!, outputFile);
      _verifyChecksumIfConfigured(defines, choice.releaseName, outputFile);
    } else {
      _copyIfNeeded(choice.localFile!, outputFile);
    }

    output.assets.code.add(
      CodeAsset(
        package: input.packageName,
        name: choice.bundleName,
        file: outputFile,
        linkMode: choice.linkMode,
      ),
    );
  });
}

final class _Defines {
  _Defines(BuildInput input)
    : releaseTag = _readString(input, 'release_tag', _defaultTag),
      githubOwner = _readString(input, 'github_owner', _defaultOwner),
      githubRepo = _readString(input, 'github_repo', _defaultRepo),
      allowStaticLinking = _readBool(input, 'allow_static_linking', false),
      checksums = _readChecksums(input);

  final String releaseTag;
  final String githubOwner;
  final String githubRepo;
  final bool allowStaticLinking;
  final Map<String, String> checksums;

  static String _readString(BuildInput input, String key, String fallback) {
    final value = input.userDefines[key];
    if (value == null) return fallback;
    if (value is String && value.trim().isNotEmpty) return value.trim();
    throw FormatException(
      'hooks.user_defines.sgtp_chat_core.$key must be a string.',
    );
  }

  static bool _readBool(BuildInput input, String key, bool fallback) {
    final value = input.userDefines[key];
    if (value == null) return fallback;
    if (value is bool) return value;
    if (value is String) {
      if (value == 'true') return true;
      if (value == 'false') return false;
    }
    throw FormatException(
      'hooks.user_defines.sgtp_chat_core.$key must be a bool.',
    );
  }

  static Map<String, String> _readChecksums(BuildInput input) {
    final raw = input.userDefines['sha256'];
    if (raw == null) return const {};
    if (raw is! Map) {
      throw const FormatException(
        'hooks.user_defines.sgtp_chat_core.sha256 must be a map.',
      );
    }
    return {
      for (final entry in raw.entries)
        if (entry.key is String && entry.value is String)
          entry.key as String: entry.value as String,
    };
  }
}

final class _Target {
  _Target(this.os, this.arch);

  factory _Target.from(CodeConfig config) =>
      _Target(config.targetOS, config.targetArchitecture);

  final OS os;
  final Architecture arch;

  String get key => '${os.name}-${arch.name}';

  _AssetChoice? chooseAsset(_Release release, {required bool allowStatic}) {
    final candidates = _candidateReleaseNames().where(
      (candidate) => allowStatic || candidate.linkMode is! StaticLinking,
    );
    for (final candidate in candidates) {
      final asset = release.assets[candidate.releaseName];
      if (asset == null) continue;
      return _AssetChoice(
        releaseName: candidate.releaseName,
        bundleName: candidate.bundleName,
        downloadUrl: asset.downloadUrl,
        localFile: null,
        linkMode: candidate.linkMode,
      );
    }
    return null;
  }

  Future<_AssetChoice?> tryBuildLocalDynamic(BuildInput input) async {
    if (os != OS.current || arch != Architecture.current) return null;
    if (os != OS.linux && os != OS.macOS && os != OS.windows) return null;

    final rustRoot = Directory.fromUri(input.packageRoot.resolve('../../'));
    if (!File('${rustRoot.path}/Cargo.toml').existsSync()) return null;

    final result = await Process.run('cargo', [
      'build',
      '--release',
      '--lib',
    ], workingDirectory: rustRoot.path);
    if (result.exitCode != 0) {
      throw ProcessException(
        'cargo',
        ['build', '--release', '--lib'],
        'Local chat_core build failed:\n${result.stdout}\n${result.stderr}',
        result.exitCode,
      );
    }

    final builtFile = File.fromUri(
      rustRoot.uri.resolve(
        'target/release/${os.dylibFileName(_libraryBaseName)}',
      ),
    );
    if (!builtFile.existsSync()) {
      throw StateError(
        'Local chat_core build did not produce ${builtFile.path}.',
      );
    }

    return _AssetChoice(
      releaseName: 'local-${builtFile.uri.pathSegments.last}',
      bundleName: os.dylibFileName(_libraryBaseName),
      downloadUrl: null,
      localFile: builtFile.uri,
      linkMode: DynamicLoadingBundled(),
    );
  }

  List<_Candidate> _candidateReleaseNames() {
    final flavor = _releaseFlavor();
    final dynamicName = os.dylibFileName(_libraryBaseName);
    final staticName = os.staticlibFileName(_libraryBaseName);

    return switch (os) {
      OS.android || OS.linux || OS.macOS => [
        _Candidate(
          'chat_core-$flavor.${_dynamicExtension(os)}',
          dynamicName,
          DynamicLoadingBundled(),
        ),
        _Candidate(
          'chat_core-$flavor.${_staticExtension(os)}',
          staticName,
          StaticLinking(),
        ),
      ],
      OS.windows => [
        _Candidate(
          'chat_core-windows-msvc-$flavor.dll',
          dynamicName,
          DynamicLoadingBundled(),
        ),
        _Candidate(
          'chat_core-windows-$flavor.dll',
          dynamicName,
          DynamicLoadingBundled(),
        ),
        _Candidate(
          'chat_core-windows-msvc-$flavor.lib',
          'chat_core.lib',
          StaticLinking(),
        ),
        _Candidate(
          'chat_core-windows-$flavor.lib',
          'chat_core.lib',
          StaticLinking(),
        ),
      ],
      OS.iOS => [
        _Candidate('chat_core-ios-$flavor.a', staticName, StaticLinking()),
      ],
      final unsupported => throw UnsupportedError(
        'chat_core does not support ${unsupported.name}.',
      ),
    };
  }

  String _releaseFlavor() {
    if (os == OS.android) {
      return switch (arch) {
        Architecture.arm64 => 'android-arm64',
        Architecture.arm => 'android-armv7',
        Architecture.x64 => 'android-amd64',
        final unsupported => throw UnsupportedError(
          'chat_core does not support Android ${unsupported.name}.',
        ),
      };
    }
    if (os == OS.linux) {
      return switch (arch) {
        Architecture.x64 => 'linux-amd64',
        Architecture.arm64 => 'linux-arm64',
        Architecture.arm => 'linux-armv7',
        final unsupported => throw UnsupportedError(
          'chat_core does not support Linux ${unsupported.name}.',
        ),
      };
    }
    if (os == OS.windows) {
      return switch (arch) {
        Architecture.x64 => 'amd64',
        Architecture.arm64 => 'arm64',
        final unsupported => throw UnsupportedError(
          'chat_core does not support Windows ${unsupported.name}.',
        ),
      };
    }
    if (os == OS.macOS) {
      return switch (arch) {
        Architecture.x64 => 'macos-amd64',
        Architecture.arm64 => 'macos-arm64',
        final unsupported => throw UnsupportedError(
          'chat_core does not support macOS ${unsupported.name}.',
        ),
      };
    }
    if (os == OS.iOS) {
      return switch (arch) {
        Architecture.arm64 => 'ios-arm64',
        Architecture.x64 => 'ios-amd64',
        final unsupported => throw UnsupportedError(
          'chat_core does not support iOS ${unsupported.name}.',
        ),
      };
    }
    throw UnsupportedError('chat_core does not support ${os.name}.');
  }

  static String _dynamicExtension(OS os) => switch (os) {
    OS.windows => 'dll',
    OS.macOS || OS.iOS => 'dylib',
    _ => 'so',
  };

  static String _staticExtension(OS os) => switch (os) {
    OS.windows => 'lib',
    _ => 'a',
  };
}

final class _Candidate {
  _Candidate(this.releaseName, this.bundleName, this.linkMode);

  final String releaseName;
  final String bundleName;
  final LinkMode linkMode;
}

final class _AssetChoice {
  _AssetChoice({
    required this.releaseName,
    required this.bundleName,
    required this.downloadUrl,
    required this.localFile,
    required this.linkMode,
  });

  final String releaseName;
  final String bundleName;
  final Uri? downloadUrl;
  final Uri? localFile;
  final LinkMode linkMode;
}

final class _Release {
  _Release(this.assets);

  final Map<String, _ReleaseAsset> assets;

  Iterable<String> get assetNames => assets.keys;

  static Future<_Release> fetch({
    required String owner,
    required String repo,
    required String tag,
  }) async {
    final url = Uri.https(
      'api.github.com',
      '/repos/$owner/$repo/releases/tags/$tag',
    );
    final response = await _readJson(url);
    final rawAssets = response['assets'];
    if (rawAssets is! List) {
      throw FormatException(
        'GitHub release $owner/$repo $tag has no assets list.',
      );
    }
    return _Release({
      for (final raw in rawAssets)
        if (raw is Map<String, Object?>)
          raw['name'] as String: _ReleaseAsset(
            name: raw['name'] as String,
            downloadUrl: Uri.parse(raw['browser_download_url'] as String),
          ),
    });
  }
}

final class _ReleaseAsset {
  _ReleaseAsset({required this.name, required this.downloadUrl});

  final String name;
  final Uri downloadUrl;
}

Future<Map<String, Object?>> _readJson(Uri url) async {
  final client = HttpClient();
  try {
    final request = await client.getUrl(url);
    request.headers.set(
      HttpHeaders.acceptHeader,
      'application/vnd.github+json',
    );
    request.headers.set(HttpHeaders.userAgentHeader, 'sgtp_chat_core_hook');
    final response = await request.close();
    final body = await utf8.decodeStream(response);
    if (response.statusCode < 200 || response.statusCode >= 300) {
      throw HttpException(
        'GitHub API request failed with HTTP ${response.statusCode}: $body',
        uri: url,
      );
    }
    final decoded = jsonDecode(body);
    if (decoded is! Map<String, Object?>) {
      throw FormatException('GitHub API response was not a JSON object: $url');
    }
    return decoded;
  } finally {
    client.close(force: true);
  }
}

Future<void> _downloadIfNeeded(Uri url, Uri outputFile) async {
  final file = File.fromUri(outputFile);
  if (file.existsSync() && file.lengthSync() > 0) return;

  file.parent.createSync(recursive: true);
  final tempFile = File('${file.path}.download');
  if (tempFile.existsSync()) tempFile.deleteSync();

  final client = HttpClient();
  try {
    final request = await client.getUrl(url);
    request.headers.set(HttpHeaders.userAgentHeader, 'sgtp_chat_core_hook');
    final response = await request.close();
    if (response.statusCode < 200 || response.statusCode >= 300) {
      final body = await utf8.decodeStream(response);
      throw HttpException(
        'Download failed with HTTP ${response.statusCode}: $body',
        uri: url,
      );
    }
    await response.pipe(tempFile.openWrite());
    if (file.existsSync()) file.deleteSync();
    tempFile.renameSync(file.path);
  } finally {
    client.close(force: true);
    if (tempFile.existsSync()) tempFile.deleteSync();
  }
}

void _copyIfNeeded(Uri source, Uri outputFile) {
  final sourceFile = File.fromUri(source);
  final output = File.fromUri(outputFile);
  if (output.existsSync() &&
      output.lengthSync() == sourceFile.lengthSync() &&
      output.lastModifiedSync().isAfter(sourceFile.lastModifiedSync())) {
    return;
  }
  output.parent.createSync(recursive: true);
  sourceFile.copySync(output.path);
}

void _verifyChecksumIfConfigured(
  _Defines defines,
  String releaseName,
  Uri outputFile,
) {
  final expected = defines.checksums[releaseName];
  if (expected == null || expected.isEmpty) return;

  final file = File.fromUri(outputFile);
  final actual = sha256.convert(file.readAsBytesSync()).toString();
  if (actual != expected.toLowerCase()) {
    file.deleteSync();
    throw StateError(
      'Checksum mismatch for $releaseName. Expected $expected, got $actual.',
    );
  }
}
