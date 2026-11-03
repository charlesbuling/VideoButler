// Copyright (C) 2026 zhengkuang <217010072@link.cuhk.edu.cn>
// SPDX-License-Identifier: AGPL-3.0-or-later
//
// This program is free software: you can redistribute it and/or modify
// it under the terms of the GNU Affero General Public License as
// published by the Free Software Foundation, either version 3 of the
// License, or (at your option) any later version.
//
// This program is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
// GNU Affero General Public License for more details.

import 'dart:convert';

import 'package:flutter_test/flutter_test.dart';
import 'package:video_butler/src/models.dart';

void main() {
  group('parsing engine responses', () {
    test('a full run result parses every section', () {
      final json = jsonDecode('''
      {
        "draftPath": "/tmp/p/sample.otio",
        "footageGraphPath": "/tmp/p/sample.footage-graph.json",
        "editingDslPath": "/tmp/p/sample.editing-dsl.json",
        "projectDir": "/tmp/p",
        "templateId": "rhythm.calm-observational",
        "durationUs": 24700000,
        "shotsDetected": 4,
        "clipsUsed": 4,
        "verified": 5,
        "rejected": 0,
        "rejections": [],
        "reusedSources": 3,
        "totalSources": 4,
        "warnings": ["MockAsr produced no real output"],
        "shots": [
          {"id":"shot-0000","startUs":0,"endUs":6000000,"durationUs":6000000,
           "score":0.901,"keyframes":2,"used":true,"description":null}
        ],
        "slots": [
          {"id":"slot-title","kind":"titlecard","durationUs":2500000,
           "timelineStartUs":0,"shotId":null,"reason":"opens on a title card",
           "score":null}
        ]
      }
      ''') as Map<String, dynamic>;

      final result = RunResult.fromJson(json);
      expect(result.templateId, 'rhythm.calm-observational');
      expect(result.shotsDetected, 4);
      expect(result.reusedSources, 3);
      expect(result.totalSources, 4);
      expect(result.warnings, hasLength(1));
      expect(result.shots.single.score, closeTo(0.901, 1e-9));
      expect(result.shots.single.used, isTrue);
      expect(result.slots.single.isTitleCard, isTrue);
    });

    test('a response missing optional fields degrades rather than throwing', () {
      // The engine gains fields over time. An older app build must keep
      // working against a newer engine rather than crashing on startup.
      final result = RunResult.fromJson(const {});
      expect(result.shots, isEmpty);
      expect(result.slots, isEmpty);
      expect(result.warnings, isEmpty);
      expect(result.durationUs, 0);
    });

    test('doctor separates mock adapters from real ones', () {
      // The UI depends on this to tell the user a cut was assembled without
      // the system having listened to or looked at the footage.
      final doctor = DoctorReport.fromJson(jsonDecode('''
      {
        "mediaBackend": "FfmpegCli",
        "mediaAvailable": true,
        "mediaVersion": "9.0.1",
        "mediaNote": "ffmpeg found on PATH.",
        "canAnalyse": true,
        "adapters": [
          {"role":"Speech","name":"MockAsr","mock":true},
          {"role":"Reasoning","name":"montage-v0","mock":false}
        ]
      }
      ''') as Map<String, dynamic>);

      expect(doctor.canAnalyse, isTrue);
      expect(doctor.adapters, hasLength(2));
      expect(doctor.mockedAdapters.single.name, 'MockAsr');
    });

    test('a template reads its parameters out of the nested params object', () {
      final template = EditTemplate.fromJson(jsonDecode('''
      {
        "id": "rhythm.upbeat-montage",
        "name": "Upbeat montage",
        "kind": "rhythm",
        "tags": ["fast", "music-led"],
        "params": {
          "targetDurationUs": 30000000,
          "minSlotUs": 1200000,
          "maxSlotUs": 3500000,
          "transition": "cut",
          "subtitles": false
        }
      }
      ''') as Map<String, dynamic>);

      expect(template.name, 'Upbeat montage');
      expect(template.tags, contains('fast'));
      expect(template.targetDurationUs, 30000000);
      expect(template.subtitles, isFalse);
    });

    test('a directory listing keeps folders and videos apart', () {
      final listing = DirectoryListing.fromJson(jsonDecode('''
      {
        "path": "/Users/me/Movies",
        "parent": "/Users/me",
        "entries": [
          {"name":"Trips","path":"/Users/me/Movies/Trips","kind":"dir"},
          {"name":"hike.mp4","path":"/Users/me/Movies/hike.mp4",
           "kind":"video","sizeBytes":2550981}
        ]
      }
      ''') as Map<String, dynamic>);

      expect(listing.error, isNull);
      expect(listing.entries.first.isDirectory, isTrue);
      expect(listing.entries.last.isDirectory, isFalse);
      expect(listing.entries.last.sizeBytes, 2550981);
    });
  });

  group('path handling', () {
    test('a posix path yields its directory', () {
      expect(parentDirectoryOf('/Users/me/Movies/hike.mp4'),
          '/Users/me/Movies');
    });

    test('a windows path yields its directory', () {
      expect(parentDirectoryOf(r'C:\Users\me\hike.mp4'), r'C:\Users\me');
    });

    test('a bare filename has no directory rather than throwing', () {
      // Reopening the picker on a path with no separator must not crash.
      expect(parentDirectoryOf('hike.mp4'), isNull);
    });

    test('a file at the root has no directory', () {
      expect(parentDirectoryOf('/hike.mp4'), isNull);
    });

    test('nothing in, nothing out', () {
      expect(parentDirectoryOf(null), isNull);
      expect(parentDirectoryOf(''), isNull);
    });
  });

  group('formatting', () {
    test('durations render as timecode', () {
      expect(formatDuration(0), '00:00.000');
      expect(formatDuration(24700000), '00:24.700');
      expect(formatDuration(90500000), '01:30.500');
    });

    test('durations past an hour include the hour', () {
      expect(formatDuration(3661000000), '01:01:01');
    });

    test('byte sizes stay readable at every scale', () {
      expect(formatBytes(512), '512 B');
      expect(formatBytes(2550981), '2.4 MB');
      expect(formatBytes(5 * 1024 * 1024 * 1024), '5.0 GB');
    });
  });
}
