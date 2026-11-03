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

/// Every string the app shows, in Chinese and English.
///
/// Written by hand rather than generated from ARB files: there are two
/// languages and a few hundred strings, and a typed class means a missing
/// translation is a compile error, not a blank label found by a user. Both
/// versions of each string sit in one file so they are read, and changed,
/// together.
///
/// Sentences the engine writes -- the self-test, which models ran, warnings,
/// why each clip was chosen -- come back already in the user's language: the
/// engine is told it through `settings.update`.
library;

import 'package:flutter/widgets.dart';

enum AppLanguage {
  zh('zh', Locale('zh', 'CN')),
  en('en', Locale('en'));

  const AppLanguage(this.id, this.locale);
  final String id;
  final Locale locale;

  S get strings => this == AppLanguage.zh ? const SZh() : const SEn();

  static AppLanguage parse(Object? value) =>
      value == 'en' ? AppLanguage.en : AppLanguage.zh;
}

/// The current language, and a way to change it, for everything below.
class LanguageScope extends InheritedWidget {
  const LanguageScope({
    super.key,
    required this.language,
    required this.onChanged,
    required super.child,
  });

  final AppLanguage language;
  final ValueChanged<AppLanguage> onChanged;

  static LanguageScope? maybeOf(BuildContext context) =>
      context.dependOnInheritedWidgetOfExactType<LanguageScope>();

  @override
  bool updateShouldNotify(LanguageScope oldWidget) => language != oldWidget.language;
}

abstract class S {
  const S();

  /// The strings for the language in scope. Chinese when nothing says
  /// otherwise, because that is the language the app is built for first.
  static S of(BuildContext context) =>
      (LanguageScope.maybeOf(context)?.language ?? AppLanguage.zh).strings;

  AppLanguage get language;

  // --- shared ------------------------------------------------------------
  String get appName;
  String get cancel;
  String get save;
  String get delete;
  String get keepIt;
  String get remove;
  String get change;
  String get automatic;
  String get ready;
  String get name;
  String get installed;
  String andMore(int n);
  String copied(String label);
  String get copyPath;
  String couldNotSave(String error);

  // --- navigation ----------------------------------------------------------
  String get navProjects;
  String get navAlbum;
  String get navModels;
  String get navSettings;
  String get navQuickDraft;

  // --- self-test -----------------------------------------------------------
  String get selfTestIntro;
  String get engineUnreachable;
  String get environment;
  String get envAllGood;
  String get envMissing;
  String get envWorthKnowing;
  String get cannotRun;
  String get checkAgain;
  String get continueLabel;

  // --- projects --------------------------------------------------------------
  String get projectsUnreadable;
  String get peopleInFootage;
  String get peopleEmptyHint;
  String peopleFound(int n);
  String get openAlbum;
  String get nobodyYet;
  String get projects;
  String get newProject;
  String get noProjects;
  String get projectNameHint;
  String get create;
  String items(int n);
  String get onTheBeat;
  String get cloud;
  String lastRunOn(String date);

  // --- a project -----------------------------------------------------------------
  String get rename;
  String get renameProject;
  String get showProjectFolder;
  String get deleteProject;
  String deleteProjectTitle(String name);
  String get deleteProjectBody;
  String get stoppedKept;
  String get runFailed;
  String get noDraftThisTime;
  String get footage;
  String get footageEmptySubtitle;
  String itemsChosen(int n);
  String get addFootage;
  String get noVideosYet;
  String get style;
  String get styleSubtitle;
  String get mood;
  String get topic;
  String moodLabel(String id);
  String topicLabel(String id);
  String get cutOnTheBeat;
  String get beatWithMusic;
  String get beatWithoutMusic;
  String get length;
  String get lengthSubtitle;
  String durationChoice(int seconds);
  String get lengthNote;
  /// What automatic means for a 主体衔接 cut: the action's own length.
  String get lengthNoteContinuity;
  /// What a 主体衔接 cut does, shown when the topic is chosen.
  String get continuityHint;
  /// What the loop switch means for a 主体衔接 cut, whose automatic length
  /// is unknown until the footage is analysed, so it is never asked.
  String get musicLoopContinuity;
  String get music;
  String get musicSubtitle;
  String get noMusic;
  String get importMusic;
  String musicFacts(String duration, double? bpm, bool steady);
  String get musicReading;
  String get models;
  String get modelsSubtitle;
  String get local;
  String get cloudApi;
  String get roleSpeech;
  String get roleVision;
  String get roleFraming;
  String get notDownloadable;
  String get framingSubtitle;
  String get noFramingModel;
  String get framingOnThisMac;
  String get noSpeechModel;
  String speechOnThisMac(String model);
  String get roleCaptions;
  String captionsOnThisMac(String model);
  String get noCaptionModel;
  String get rolePose;
  String poseOnThisMac(String model);
  String get noPoseModel;
  String get getAModel;
  String visionOverEndpoint(String model, String url);
  String get visionNone;
  String get visionApple;
  String get visionSiglip;
  String get noEndpoint;
  String get noEndpointBody;
  String get cloudSettings;
  String get whereDraftGoes;
  String get whereDraftGoesSubtitle;
  String get editorNotInstalled;
  String editorOnPhone(String name);
  String handOffTitle(String name);
  String handOffBody(int clips, String name);
  String handOffLeftOut(int titleCards, int texts, int transitions);
  String handOffMusic(String music, String start);
  String handOffButton(String name);
  String handOffCutting(int done, int total);
  String handOffDone(String name, String folder);
  String handOffFailed(String name, String why);
  String get editorNoFolder;
  String get editorRunning;
  String get editorReady;
  String get otioDraft;
  String get otioAlways;
  String get createDraft;
  String createDraftFor(String editor);
  String get lastRun;
  String get durationStat;
  String get clipsStat;
  String get draftLabel;
  String get editorDraftLabel;

  // --- progress ------------------------------------------------------------------
  String get starting;
  String stageLabel(String id, String fallback);
  String clipOf(int index, int count);
  String clipsAnalysing(int done, int count, int active);

  // --- analysis, music length ---------------------------------------------------
  String get analysisReuse;
  String get analysisFresh;
  String get analysisChoiceNote;
  String get musicShortTitle;
  String musicShortBody(String music, String cut);
  String get loopMusic;
  String get dontLoopMusic;
  String get musicLoopSwitch;
  String get musicLoopSwitchNote;
  String stepLabel(String step, String? file, String fallback);
  String get stop;
  String get stopping;

  // --- result ------------------------------------------------------------------------
  String get roughStructure;
  String get readyInEditor;
  String get readyInEditorSubtitle;
  String draftFacts(int clips, bool music, int transitionsDropped, int linked, int unreadable);
  String get showDraftFolder;
  String openEditor(String editor);
  String get restartEditorNote;
  String get howItWasMade;
  String get result;
  String styleName(String templateId, String fallback);
  String cutOnBeat(String bpmText);
  String beatDescription(String origin, double bpm);
  String get reusedAnalysis;
  String reusedOf(int x, int y);
  String get shotsFound;
  String get clipsUsed;
  String get videos;
  String get peopleFoundStat;
  String get grounded;
  String get rejected;
  String get theCut;
  String get theCutSubtitle;
  String get titleCard;
  String get everyShot;
  String get everyShotSubtitle;
  String get rejectedByGrounding;
  String get rejectedByGroundingSubtitle;
  String get filesWritten;
  String get filesWrittenSubtitle;
  String get footageGraph;
  String get editingDsl;

  // --- file browser ------------------------------------------------------------
  String get parentDirectory;
  String get chooseVideos;
  String get chooseMusic;
  String get useThisFolder;
  String videosChosen(int n);
  String get clear;
  String get useThese;
  String get noMusicHere;
  String get close;
  String get thermalTitle;
  String get thermalSwitch;
  String get thermalNote;
  String get thermalSlowing;
  String get thermalCooling;
  String get previewClip;
  String get previewFailed;
  String get chooseClip;
  String get chosenClip;
  String placeLabel(String id);
  String get noMusicOnPhone;
  String get noVideosHere;
  String get selectAll;
  String get deselectAll;
  String shownOf(int shown, int total);
  String get readingClips;
  String get filterDate;
  String get filterRatio;
  String get filterKind;
  String kindOption(String kind, int n);
  String footageChosen(int videos, int photos);
  String get previewPhoto;
  String get chooseThisPhoto;
  String get importFromPhotos;
  String get importFromPhotosNote;
  String get browseAppFiles;
  String get allOption;
  String get unknownDate;
  String dayLabel(DateTime day);
  String monthLabel(DateTime month);
  String orientationLabel(String orientation);
  String get fileDateNote;
  String get noMatches;

  // --- models page --------------------------------------------------------------
  String get modelsUnreadable;
  String get downloadedOnce;
  String modelsKeptIn(String dir);
  String get visionNeedsNoDownload;
  String get visionOnPhone;
  String get speechSubtitle;
  String get download;
  String deleteModelTitle(String name);
  String deleteModelBody(String size);
  String downloadFailed(String error);

  // --- settings --------------------------------------------------------------------
  String get cacheTitle;
  String cacheSubtitle(int entries, String size);
  String get cacheEmpty;
  String get clearCache;
  String get clearCacheTitle;
  String get clearCacheBody;
  String cacheCleared(String size);
  String get draftMediaTitle;
  String get draftMediaSubtitle;
  String get draftMediaLink;
  String get draftMediaLinkNote;
  String get draftMediaReference;
  String get draftMediaReferenceNote;
  String get settingsUnreadable;
  String get languageTitle;
  String get cloudModel;
  String get cloudModelSubtitle;
  String get setUp;
  String get notSetUp;
  String get baseUrl;
  String get model;
  String get modelHint;
  String get visionModel;
  String get visionModelHint;
  String get apiKey;
  String get apiKeyStored;
  String get apiKeyNotNeeded;
  String get forgetKey;
  String get keyPrivacy;
  String get testConnection;
  String get saved;
  String get endpointAnswered;
  String get endpointNoList;
  String endpointHasModel(int n);
  String endpointLacksModel(int n, String sample);
  String get endpointSilent;
  String get noReason;
  String get library;
  String get librarySubtitle;
  String get showInFinder;

  // --- album ---------------------------------------------------------------------------
  String get albumUnreadable;
  String get albumNameHint;
  String get albumEmpty;
  String get whoIsThis;
  String get aQuickRun;
  String shots(int n);
  String get notAPerson;
  String get importModels;
  String get importModelsNote;
  String get chooseFolder;
  String get importPicking;
  String importDone(int imported, int already);
  String get importNothing;
  String importFailed(String files);
  String importTooBig(String models);
  String get captionsAndDance;
  String get captionsAndDanceSubtitle;
  String get roleLabels;
  String get labelsSubtitle;
  String get tooBigForDevice;
  String get bundledModel;
  String get inUse;
  String get useModel;
  String subjectAppearsIn(int projects, int clips, int shots);
  String clipsAndShots(int clips, int shots);
  String get openProject;
  String get clipNotRecorded;
  String get forgetSubjectTitle;
  String get forgetSubjectBody;
  String get unnamed;
  String clips(int n);

  // --- quick draft ----------------------------------------------------------------------
  String get reloadEngine;
  String get serveHint;
  String get engine;
  String get cannotAnalyse;
  String get analysisUnavailable;
  String get perceptionAdapters;
  String adapterChip(String role, String name);
  String get mockNote;
  String sourceVideos(int n);
  String get choose;
  String get editingStyle;
  String get chosenFromFootage;
  String get template;
  String target(String duration);
  String clipsRange(String from, String to);
  String get subtitles;
  String get analysing;
  String get createADraft;
  String get theRunFailed;

  // --- the video butler ----------------------------------------------------
  String get hostingCta;
  String get hostingIntro;
  String get hostingStop;
  String get hostingAgain;
  String get hostingRunning;
  String hostingStatus(int videos, int photos, String? when, int memories);
  String get hostingNever;
  String hostingStep(String step, String? file, int index, int count);
  String get hostingFolders;
  String get hostingFoldersDefault;
  String get changeFolders;
  String get addFolder;
  String get useDefaultFolders;
  String get disksPluggedIn;
  String get foldersUnreachable;
  String get tabCategories;
  String get tabMemoryMap;
  String get sectionFaces;
  String get facesSubtitle;
  String get facesEmpty;
  String get sectionSmart;
  String get smartSubtitle;
  String get smartEmpty;
  String videoCount(int n);

  /// Videos and photos together, the photos left out when there are none.
  String footageCount(int videos, int photos);
  String get unverifiedBadge;
  String get removedCategories;
  String get restore;
  String get removeCategory;
  String removeCategoryTitle(String name);
  String get removeCategoryBody;
  String get selectNone;
  String selectedCount(int n);
  String newProjectFrom(int videos, [int photos = 0]);
  String get gridEmpty;
  String get fileTimeMark;
  String get hostedVideos;
  String get seeInButler;
  String get mapAllMemories;
  String memoryKindLabel(MemoryKindId kind);
  String get mapEmpty;
  String unpinnedMemories(int n);
  String memoryStatus(String? status, bool run);
  String get generateProject;
  String get regenerate;
  String get orderLabel;
  String get orderTime;
  String get orderCategory;
  String memoryDates(String start, String end);
  String get memoryProjectNote;
  String get mapAttribution;
  String memoriesHere(int n);
  String get mapLayers;
  String mapStats(int memories, int places, int regions);
  String get mapResetView;
  String get memoryGenerating;
  String get mergePerson;
  String get butlerNotesTitle;
  String get memoryBadge;
  String get mergeTitle;
  String get mergeBody;

  // --- the built-in editor (0.2.0) -------------------------------------------
  String get edCardTitle;
  String get edCardBody;
  String get edEditInApp;
  String get edExportProject;
  String edEditedOn(String date);
  String get edExportsTitle;
  String edExportKind(String kind);
  String get edOpening;
  String edEdited(int percent);
  String get edAsMade;
  String get edUndo;
  String get edRedo;
  String get edExport;
  String get edTitle;
  String get edRevertAi;
  String get edRevertAiBody;
  String get edRestartFromRun;
  String get edRestartBody;
  String get edNewerRun;
  String get edPlay;
  String get edPause;
  String get edPrevFrame;
  String get edNextFrame;
  String get edZoomIn;
  String get edZoomOut;
  String get edBack;
  String get edSplit;
  String get edDelete;
  String get edSpeed;
  String get edVolume;
  String get edDuration;
  String get edAdjust;
  String get edPosition;
  String get edBackground;
  String get edBackgroundAll;
  String get edDuplicate;
  String get edMoveEarlier;
  String get edMoveLater;
  String get edTransition;
  String get edTransitionAll;
  String get edEditText;
  String get edTextToPlayhead;
  String get edReplaceMusic;
  String get edAddClip;
  String get edText;
  String get edMusic;
  String get edCanvas;
  String get edSoundOff;
  String get edSoundOn;
  String get edNewText;
  String get edSpeedNote;
  String get edMute;
  String get edMuteAll;
  String get edTransCut;
  String get edTransXfade;
  String get edTransFade;
  String get edTransWipeLeft;
  String get edTransWipeRight;
  String get edLength;
  String get edApplyToAll;
  String get edReset;
  String get edBrightness;
  String get edContrast;
  String get edSaturation;
  String get edScale;
  String get edMoveX;
  String get edMoveY;
  String get edRotate;
  String get edRotate90;
  String get edRatio;
  String get edRatioOriginal;
  String get edBgBlack;
  String get edBgBlur;
  String get edTextHint;
  String get edDone;
  String get edFontSize;
  String get edBold;
  String get edOutline;
  String get edTextBox;
  String get edPlace;
  String get edPlaceTop;
  String get edPlaceMiddle;
  String get edPlaceBottom;
  String get edDragTextHint;
  String get edFadeIn;
  String get edFadeOut;
  String get edRemoveMusic;
  String get edFootageNote;
  String edUsedTimes(int n);
  String get edNoStretches;
  String get edExportVideo;
  String get edLosslessTitle;
  String get edLosslessChecking;
  String get edLosslessButton;
  String edLosslessNote(double extraSeconds);
  String get edLosslessBlocked;
  String get edClipsButton;
  String get edClipsNote;
  String edClipsDone(int copied, int rendered);
  String get edNoVideoExport;
  String get edResolution;
  String get edFrameRate;
  String get edFrameRateSource;
  String get edCodec;
  String get edCodecH264;
  String get edCodecHevc;
  String get edExportDrafts;
  String get edDraftNote;
  String get edDraftJianying;
  String get edDraftCapcut;
  String get edDraftOtio;
  String get edDraftOtioNote;
  String edEditorMissing(String name);
  String get edExporting;
  String edFrames(int done, int total);
  String get edThermal;
  String get edExported;
  String get edExportedTo;
  String get edShowInFolder;
  String edExportFailed(String error);
}

/// The kinds of memory, by the engine's id, for words that depend on it.
enum MemoryKindId { travel, growth, annual, group }

class SZh extends S {
  const SZh();

  @override
  AppLanguage get language => AppLanguage.zh;

  @override
  String get cacheTitle => '分析缓存';
  @override
  String cacheSubtitle(int entries, String size) =>
      '保存了之前分析的结果（$entries 条，$size）。再次处理同样的视频时直接复用，不用重新分析。';
  @override
  String get cacheEmpty => '还没有保存任何分析结果。';
  @override
  String get clearCache => '清除全部分析';
  @override
  String get clearCacheTitle => '清除全部分析？';
  @override
  String get clearCacheBody =>
      '之后处理任何视频都要从头分析。项目、已生成的草稿、视频管家的人物和下载的模型都不受影响。';
  @override
  String cacheCleared(String size) => '已清除，释放 $size';
  @override
  String get draftMediaTitle => '草稿引用素材的方式';
  @override
  String get draftMediaSubtitle => '剪映和 CapCut 在 Mac 上只能读“影片”文件夹里的文件和你在它们里面亲自选过的文件';
  @override
  String get draftMediaLink => '硬链接（推荐）';
  @override
  String get draftMediaLinkNote =>
      '在草稿的 Resources 里放一个指向原文件的硬链接：同一份数据，不复制、不占额外空间（访达会把大小重复算一次），剪映能直接打开。';
  @override
  String get draftMediaReference => '只引用原文件';
  @override
  String get draftMediaReferenceNote =>
      '草稿里只写原文件路径。素材不在“影片”文件夹时，剪映会显示“暂无访问权限”，需要在剪映里重新链接。';


  @override
  String get appName => 'Video Butler';
  @override
  String get cancel => '取消';
  @override
  String get save => '保存';
  @override
  String get delete => '删除';
  @override
  String get keepIt => '保留';
  @override
  String get remove => '移除';
  @override
  String get change => '更换';
  @override
  String get automatic => '自动';
  @override
  String get ready => '就绪';
  @override
  String get name => '名称';
  @override
  String get installed => '已安装';
  @override
  String andMore(int n) => '还有 $n 项';
  @override
  String copied(String label) => '已复制$label';
  @override
  String get copyPath => '复制路径';
  @override
  String couldNotSave(String error) => '保存失败：$error';

  @override
  String get navProjects => '项目';
  @override
  String get navAlbum => '视频管家';
  @override
  String get navModels => '模型';
  @override
  String get navSettings => '设置';
  @override
  String get navQuickDraft => '快速剪辑';

  @override
  String get selfTestIntro => '第一次运行前，先检查一下这台设备。';
  @override
  String get engineUnreachable => '无法连接引擎';
  @override
  String get environment => '运行环境';
  @override
  String get envAllGood => '一切就绪';
  @override
  String get envMissing => '缺少运行必需的组件';
  @override
  String get envWorthKnowing => '可以运行，有几点值得注意';
  @override
  String get cannotRun => '无法运行';
  @override
  String get checkAgain => '重新检查';
  @override
  String get continueLabel => '继续';

  @override
  String get projectsUnreadable => '无法读取项目';
  @override
  String get peopleInFootage => '素材里的人物';
  @override
  String get peopleEmptyHint => '之前项目里出现过的人脸会显示在这里，按长相相似程度分组';
  @override
  String peopleFound(int n) => '在你的项目里找到 $n 个人';
  @override
  String get openAlbum => '打开视频管家';
  @override
  String get nobodyYet => '还没有人物。';
  @override
  String get projects => '项目';
  @override
  String get newProject => '新建项目';
  @override
  String get noProjects => '还没有项目。一个项目就是：一些素材、一种风格，以及草稿要送到哪里。';
  @override
  String get projectNameHint => '例如：春天的京都';
  @override
  String get create => '创建';
  @override
  String items(int n) => '$n 项素材';
  @override
  String get onTheBeat => '卡点';
  @override
  String get cloud => '云端';
  @override
  String lastRunOn(String date) => '上次运行 $date';

  @override
  String get rename => '重命名';
  @override
  String get renameProject => '重命名项目';
  @override
  String get showProjectFolder => '打开项目文件夹';
  @override
  String get deleteProject => '删除项目';
  @override
  String deleteProjectTitle(String name) => '删除「$name」？';
  @override
  String get deleteProjectBody =>
      '会删除这个项目，以及它写在自己文件夹里的文件。你的原始视频不会被动，已经在剪辑软件里的草稿会保留，其他项目共用的分析结果也会保留。';
  @override
  String get stoppedKept => '已停止。已经分析完的素材会保留，再次运行会接着来。';
  @override
  String get runFailed => '运行失败。';
  @override
  String get noDraftThisTime => '这次没有生成草稿';
  @override
  String get footage => '素材';
  @override
  String get footageEmptySubtitle => '可以选多个视频和照片，也可以选整个文件夹，文件夹里的子文件夹也会一起读取。';
  @override
  String itemsChosen(int n) => '已选 $n 项';
  @override
  String get addFootage => '添加视频、照片或文件夹';
  @override
  String get noVideosYet => '还没有选素材。视频和照片都可以，可以多选，也可以选整个文件夹。';
  @override
  String get style => '风格';
  @override
  String get styleSubtitle => '情绪决定节奏，主题决定结构';
  @override
  String get mood => '情绪';
  @override
  String get topic => '主题';
  @override
  String moodLabel(String id) =>
      const {'happy': '欢快', 'sad': '伤感', 'humor': '幽默'}[id] ?? id;
  @override
  String topicLabel(String id) =>
      const {
        'dance': '舞蹈',
        'travel': '旅行',
        'growth': '成长',
        'continuity': '主体衔接',
        'annual': '年度回顾',
        'together': '人群记忆',
      }[id] ??
      id;
  @override
  String get cutOnTheBeat => '卡点剪辑';
  @override
  String get beatWithMusic => '每个剪辑点都落在配乐的节拍上。';
  @override
  String get beatWithoutMusic => '每个剪辑点都落在节拍上：素材自带清晰的音乐就跟着它，没有就用固定节奏。';
  @override
  String get length => '时长';
  @override
  String get lengthSubtitle => '成片的目标长度';
  @override
  String durationChoice(int seconds) =>
      seconds < 60 ? '$seconds 秒' : '${seconds ~/ 60} 分钟';
  @override
  String get lengthNote => '“自动”由风格决定，大约 25 秒到 1 分钟。素材不够时，成片会短一些。';
  @override
  String get lengthNoteContinuity =>
      '“自动”就是完整动作的长度。卡点时先让舞步落在拍上（快慢都不超过 1.2 倍），选的时长比动作短就每段按同样比例缩短；不卡点时先整体加速（最多 1.2 倍），还放不下再缩短。';
  @override
  String get continuityHint =>
      '把几段素材里同一套动作拼成一整段：换镜头时，人的位置、大小和动作接着上一段，只换场景。打开卡点时先认出舞蹈自己的节拍，有人停着超过一拍半、或拿着手机在看（学动作）的地方都会剔除，大部分时间不在跳的素材整段不用。画面会按需放大、平移、微转来对齐，剪映里都能改回。目前只认人，需要姿态模型 RTMO-s。';
  @override
  String get musicLoopContinuity => '配乐比动作短时：打开就从第一拍重复，不打开配乐放完后就是静音。动作不会为了配乐加快。';
  @override
  String get music => '配乐';
  @override
  String get musicSubtitle => '铺在成片下面；打开卡点时，每个剪辑点都落在它的节拍上';
  @override
  String get noMusic => '没有配乐，草稿保留素材原声。';
  @override
  String get importMusic => '导入音乐';
  @override
  String musicFacts(String duration, double? bpm, bool steady) => bpm != null && steady
      ? '$duration · ${bpm.toStringAsFixed(0)} BPM'
      : '$duration · 没有明显的节拍';
  @override
  String get musicReading => '正在分析节拍…';
  @override
  String get models => '模型';
  @override
  String get modelsSubtitle => '本地：全部在这台设备上完成。云端：把关键帧截图发给你设置的接口。';
  @override
  String get local => '本地';
  @override
  String get cloudApi => '云端（API）';
  @override
  String get roleSpeech => '语音';
  @override
  String get roleVision => '画面';
  @override
  String get roleFraming => '构图';
  @override
  String get notDownloadable => '暂不提供下载';
  @override
  String get framingSubtitle => '三个小模型一起看画面：景深找出紧贴镜头的东西，人脸和人像分清那是人还是遮挡。都在本机运行。';
  @override
  String get noFramingModel => '还没有构图模型，被车门、仪表台挡住的画面挑不出来。';
  @override
  String get framingOnThisMac => '景深、人脸、人像，在本机运行';
  @override
  String get noSpeechModel => '没有语音模型。字幕按画面内容写，不需要它；只有访谈类风格会把说话转成字幕。';
  @override
  String speechOnThisMac(String model) => '$model，在本机运行';
  @override
  String get roleCaptions => '字幕';
  @override
  String captionsOnThisMac(String model) => '$model 按画面内容写字幕，在本机运行';
  @override
  String get noCaptionModel => '没有画面描述模型，成片不加字幕（不会用语音转文字）';
  @override
  String get rolePose => '姿态';
  @override
  String poseOnThisMac(String model) => '$model：舞蹈只选跳舞的画面，并衔接动作';
  @override
  String get noPoseModel => '没有姿态模型：舞蹈风格只能按画面动感挑选';
  @override
  String get getAModel => '下载模型';
  @override
  String visionOverEndpoint(String model, String url) => '$model（$url）';
  @override
  String get visionNone => '这个版本不识别画面内容。';
  @override
  String get visionApple => '本机 Apple Vision：场景标签和人脸';
  @override
  String get visionSiglip => 'SigLIP 2 在本机识别场景标签（这里不做人脸分组）';
  @override
  String get noEndpoint => '还没有设置云端接口';
  @override
  String get noEndpointBody => '设置之前，选“云端”也会在本机完成，并会在结果里注明。';
  @override
  String get cloudSettings => '云端接口设置';
  @override
  String get whereDraftGoes => '草稿送到哪里';
  @override
  String get whereDraftGoesSubtitle => '这里决定怎么剪，剪辑软件负责真正去剪';
  @override
  String get editorNotInstalled => '这台设备上没有安装';
  @override
  String editorOnPhone(String name) =>
      '这台设备上已安装，但手机版$name把草稿存在它自己的存储里，安卓不允许其他 App 写入，所以不能直接生成它的草稿。';
  @override
  String handOffTitle(String name) => '在$name里继续剪';
  @override
  String handOffBody(int clips, String name) =>
      '把这 $clips 段按剪好的顺序和长度交给$name，它会打开一个新项目。请先回到$name的首页：'
      '它如果正打开着某个项目，这些片段会加进那个项目。';
  @override
  String handOffLeftOut(int titleCards, int texts, int transitions) {
    final parts = [
      if (titleCards > 0) '标题卡 $titleCards 个',
      if (texts > 0) '文字 $texts 条',
      if (transitions > 0) '转场 $transitions 处',
    ];
    return parts.isEmpty ? '' : '带不过去：${parts.join('、')}。';
  }

  @override
  String handOffMusic(String music, String start) =>
      '配乐要在剪映里自己加：$music，从 $start 处开始，剪辑点才落在节拍上。';
  @override
  String handOffButton(String name) => '在$name中打开';
  @override
  String handOffCutting(int done, int total) => '正在裁剪片段 $done / $total';
  @override
  String handOffDone(String name, String folder) =>
      '已交给$name。裁好的片段存在 $folder，$name的项目要用到它们，剪完之前不要删。';
  @override
  String handOffFailed(String name, String why) => '没能交给$name：$why';
  @override
  String get editorNoFolder => '已安装，但还没有草稿文件夹，请先打开它一次。';
  @override
  String get editorRunning => '正在运行：新草稿要退出并重新打开后才会出现';
  @override
  String get editorReady => '草稿会出现在它的项目列表里，可以直接编辑';
  @override
  String get otioDraft => 'OpenTimelineIO 草稿';
  @override
  String get otioAlways => '总会写到项目文件夹，供其他剪辑软件使用';
  @override
  String get createDraft => '生成草稿';
  @override
  String createDraftFor(String editor) => '生成草稿并送到$editor';
  @override
  String get lastRun => '上次运行';
  @override
  String get durationStat => '时长';
  @override
  String get clipsStat => '片段';
  @override
  String get draftLabel => '草稿';
  @override
  String get editorDraftLabel => '剪辑软件草稿';

  @override
  String get starting => '准备中';
  @override
  String stageLabel(String id, String fallback) =>
      const {
        'prepare': '整理素材',
        'analyse': '分析画面',
        'music': '识别节拍',
        'compose': '选择片段',
        'export': '写入草稿',
        'done': '完成',
        'download': '下载模型',
      }[id] ??
      fallback;
  @override
  String clipOf(int index, int count) => '第 $index / $count 个素材';
  @override
  String clipsAnalysing(int done, int count, int active) =>
      '已完成 $done / $count 个素材${active > 1 ? ' · $active 个同时分析' : ''}';

  @override
  String get analysisReuse => '沿用已有分析';
  @override
  String get analysisFresh => '重新分析';
  @override
  String get analysisChoiceNote =>
      '沿用：分析过的视频直接复用，只分析新加入的。重新分析：所有视频从头再看一遍，用时较长。';
  @override
  String get musicShortTitle => '配乐不够长';
  @override
  String musicShortBody(String music, String cut) =>
      '配乐只有 $music，成片最长 $cut，配乐放完后的部分会没有声音。要重复播放这首歌吗？\n重复时从第一拍接着放，节拍不会乱。这个选择会记在项目里，以后可以在“配乐”里改。';
  @override
  String get loopMusic => '重复播放';
  @override
  String get dontLoopMusic => '不重复，后面静音';
  @override
  String get musicLoopSwitch => '配乐不够长时重复播放';
  @override
  String get musicLoopSwitchNote => '从第一拍接着放，节拍不会乱';
  @override
  String stepLabel(String step, String? file, String fallback) {
    const steps = {
      'prepare': '整理素材',
      'prepare.list': '读取文件列表',
      'analyse.probe': '读取视频信息',
      'analyse.shotDetect': '切分镜头',
      'analyse.keyframes': '从头到尾看画面并分段',
      'analyse.cvScore': '评分',
      'analyse.director': '看构图，找出被挡住的画面',
      'analyse.asr': '识别语音',
      'analyse.describe': '识别画面内容',
      'analyse.track': '寻找人物',
      'analyse.minePriors': '统计',
      'analyse.cached': '之前分析过，直接复用',
      'analyse.done': '分析完成',
      'prepare.hash': '读取文件，看哪些之前分析过',
      'prepare.read': '读取文件（重新分析，不用以往结果）',
      'music.listen': '分析配乐的节拍',
      'music.footage': '分析素材自带的节拍',
      'compose.choose': '挑选片段、排出时间线',
      'compose.judge': '构图模型给候选画面打分',
      'compose.dance': '读舞者的姿态，找动作衔接点',
      'compose.continuity': '在每段素材里跟住主体，对齐同一套动作',
      'compose.caption': '看画面写字幕',
      'export.otio': '写入草稿',
      'export.editor.jianying': '写入剪映草稿',
      'export.editor.capcut': '写入 CapCut 草稿',
      'done': '完成',
    };
    final label = steps[step];
    if (label == null) return fallback;
    return file == null ? label : '$file · $label';
  }

  @override
  String get stop => '停止';
  @override
  String get stopping => '正在停止…';

  @override
  String get roughStructure => '这只是结构草稿，还不是成片';
  @override
  String get readyInEditor => '已送到剪辑软件';
  @override
  String get readyInEditorSubtitle => '剪辑决定都在草稿里，由剪辑软件去剪；每个片段、剪切点和标题都还能在那里修改。';
  @override
  String draftFacts(int clips, bool music, int transitionsDropped, int linked, int unreadable) => [
        '$clips 个片段',
        if (music) '含配乐',
        if (transitionsDropped > 0) '$transitionsDropped 个转场改为硬切',
        if (linked > 0) '$linked 个素材以硬链接放进草稿（同一份文件，不占额外空间），剪辑软件才有权限读取',
        if (unreadable > 0) '$unreadable 个素材按原路径引用，剪辑软件可能要求重新链接',
      ].join('，');
  @override
  String get showDraftFolder => '打开草稿文件夹';
  @override
  String openEditor(String editor) => '打开$editor';
  @override
  String get restartEditorNote => '如果剪辑软件已经开着，请退出后重新打开：它只在启动时读取草稿文件夹。';
  @override
  String get howItWasMade => '这一版是怎么做的';
  @override
  String get result => '结果';
  @override
  String styleName(String templateId, String fallback) {
    const names = {
      'rhythm.upbeat-montage': '快节奏混剪',
      'rhythm.calm-observational': '舒缓观察',
      'rhythm.dialogue-led': '对白为主',
      'emotion.warm-nostalgic': '温暖怀旧',
      'emotion.cool-clinical': '冷静克制',
      'emotion.dramatic-high-contrast': '戏剧张力',
      'emotion.happy-bright': '欢快明亮',
      'emotion.sad-tender': '伤感温柔',
      'emotion.humor-snappy': '幽默轻快',
      'content.travel-diary': '旅行日记',
      'content.interview-cutdown': '访谈精剪',
      'content.product-showcase': '产品展示',
      'content.dance-beat': '舞蹈卡点',
      'content.growth-story': '成长故事',
      'content.annual-review': '年度回顾',
      'content.together-story': '一起走过',
    };
    if (names.containsKey(templateId)) return names[templateId]!;
    if (templateId.startsWith('style.')) {
      final parts = templateId.split('.').skip(1).map((part) {
        if (part == 'beat') return '卡点';
        final mood = moodLabel(part);
        return mood != part ? mood : topicLabel(part);
      });
      return parts.join(' · ');
    }
    return fallback.isEmpty ? templateId : fallback;
  }

  @override
  String cutOnBeat(String bpmText) => '踩着$bpmText';
  @override
  String beatDescription(String origin, double bpm) => switch (origin) {
        'music' => '配乐的 ${bpm.toStringAsFixed(0)} BPM 节拍',
        'source' => '素材自带的 ${bpm.toStringAsFixed(0)} BPM 节拍',
        _ => '固定的 ${bpm.toStringAsFixed(0)} BPM 节奏（没有选配乐）',
      };
  @override
  String get reusedAnalysis => '复用了分析结果';
  @override
  String reusedOf(int x, int y) => '复用了 $x/$y 个素材的分析';
  @override
  String get shotsFound => '找到镜头';
  @override
  String get clipsUsed => '用到片段';
  @override
  String get videos => '视频';
  @override
  String get peopleFoundStat => '找到人物';
  @override
  String get grounded => '核验通过';
  @override
  String get rejected => '被拒绝';
  @override
  String get theCut => '剪辑方案';
  @override
  String get theCutSubtitle => '按顺序列出，并说明每个片段为什么被选中';
  @override
  String get titleCard => '标题卡';
  @override
  String get everyShot => '找到的全部镜头';
  @override
  String get everyShotSubtitle => '包括没被选中的，方便你判断取舍';
  @override
  String get rejectedByGrounding => '没有通过素材核验';
  @override
  String get rejectedByGroundingSubtitle => '这些剪辑决定对不上真实的素材，已直接丢弃，而不是强行修正';
  @override
  String get filesWritten => '写出的文件';
  @override
  String get filesWrittenSubtitle => '这份草稿可以用任何支持 OpenTimelineIO 的软件打开';
  @override
  String get footageGraph => '素材分析';
  @override
  String get editingDsl => '剪辑方案文件';

  @override
  String get parentDirectory => '上一级';
  @override
  String get chooseVideos => '选择视频和照片';
  @override
  String get chooseMusic => '选择音乐';
  @override
  String get useThisFolder => '使用这个文件夹';
  @override
  String videosChosen(int n) => '已选 $n 个视频';
  @override
  String get clear => '清空';
  @override
  String get useThese => '使用所选';
  @override
  String get noMusicHere => '这里没有文件夹或音乐。\n用上方的箭头回到上一级。';
  @override
  String get close => '关闭';
  @override
  String get thermalTitle => '设备温度';
  @override
  String get thermalSwitch => '发热时自动放慢';
  @override
  String get thermalNote =>
      '处理时设备变热，就先放慢：歇一会儿再干，一次只分析一段；太热就暂停，等降下来再继续。'
      '用的是系统自己的温度判断，赶在系统因过热关掉软件之前。关掉则一直全速运行。';
  @override
  String get thermalSlowing => '设备有点热，已放慢速度，避免过热';
  @override
  String get thermalCooling => '设备太热，暂停一会儿等它降温，之后自动继续';
  @override
  String get previewClip => '播放预览';
  @override
  String get previewFailed => '这段视频没法在这里播放。它仍然可以被选中和分析。';
  @override
  String get chooseClip => '选中这段';
  @override
  String get chosenClip => '已选中';
  @override
  String placeLabel(String id) =>
      const {
        'music': '音乐',
        'downloads': '下载',
        'home': '个人文件夹',
        'desktop': '桌面',
        'documents': '文稿',
        'movies': '影片',
        'pictures': '图片',
        'dcim': '相机',
        'netease': '网易云音乐',
        'qqmusic': 'QQ音乐',
        'kugou': '酷狗音乐',
        'kuwo': '酷我音乐',
      }[id] ??
      id;
  @override
  String get noMusicOnPhone =>
      '这里没有能用的歌。如果手机里明明有，请到系统设置 → 应用 → Video Butler → 权限，允许“音乐和音频”。'
      '会员下载的加密歌曲（.ncm、.qmc、.mflac、.kgm 等）只能在原来的音乐 App 里播放，这里看不到也用不了。';
  @override
  String get noVideosHere => '这里没有文件夹、视频或照片。\n用上方的箭头回到上一级，或者直接使用整个文件夹。';
  @override
  String get deselectAll => '取消全选';
  @override
  String shownOf(int shown, int total) =>
      shown == total ? '$total 个素材' : '显示 $shown / $total 个素材';
  @override
  String get readingClips => '正在读取拍摄时间和画面比例…';
  @override
  String get filterDate => '拍摄日期';
  @override
  String get filterRatio => '画面比例';
  @override
  String get filterKind => '类型';
  @override
  String kindOption(String kind, int n) => '${kind == 'image' ? '照片' : '视频'} · $n';
  @override
  String footageChosen(int videos, int photos) => '已选 ${[
        if (videos > 0) '$videos 个视频',
        if (photos > 0) '$photos 张照片',
      ].join('、')}';
  @override
  String get previewPhoto => '查看照片';
  @override
  String get chooseThisPhoto => '选中这张';
  @override
  String get importFromPhotos => '从照片图库导入';
  @override
  String get importFromPhotosNote => '选中的视频和照片会拷贝到本 App 里分析，不会上传。';
  @override
  String get browseAppFiles => '浏览本 App 里的文件';
  @override
  String get allOption => '全部';
  @override
  String get unknownDate => '日期未知';
  @override
  String dayLabel(DateTime day) => '${day.month}月${day.day}日';
  @override
  String monthLabel(DateTime month) => '${month.year}年${month.month}月';
  @override
  String orientationLabel(String orientation) =>
      const {
        'landscape': '横拍',
        'portrait': '竖拍',
        'square': '方形'
      }[orientation] ??
      '比例未知';
  @override
  String get fileDateNote => '（文件日期）';
  @override
  String get noMatches => '没有符合筛选条件的素材。';

  @override
  String get modelsUnreadable => '无法读取模型库';
  @override
  String get downloadedOnce => '下载一次，所有项目通用';
  @override
  String modelsKeptIn(String dir) => '模型保存在 $dir。';
  @override
  String get visionNeedsNoDownload =>
      '在 Mac 上，场景标签和人脸不需要下载：系统自带 Apple Vision。字幕由画面描述模型按画面内容写；语音模型只给访谈类风格转写说话。';
  @override
  String get visionOnPhone =>
      '场景标签由 SigLIP 2 在本机识别，安装包自带；人脸分组靠 Apple Vision，只有 Mac 上有。字幕由画面描述模型按画面内容写；语音模型只给访谈类风格转写说话。';
  @override
  String get speechSubtitle => 'whisper.cpp，多语言，支持中文';
  @override
  String get download => '下载';
  @override
  String deleteModelTitle(String name) => '删除 $name？';
  @override
  String deleteModelBody(String size) => '会释放 $size。重新下载之前，所有项目都不会再用它。';
  @override
  String downloadFailed(String error) => '下载失败：$error';

  @override
  String get settingsUnreadable => '无法读取设置';
  @override
  String get languageTitle => '界面语言';
  @override
  String get cloudModel => '云端模型';
  @override
  String get cloudModelSubtitle => '任何兼容 OpenAI 的接口：在线 API，或者本机的模型服务（例如 http://localhost:11434/v1）';
  @override
  String get setUp => '已设置';
  @override
  String get notSetUp => '未设置';
  @override
  String get baseUrl => '接口地址';
  @override
  String get model => '模型';
  @override
  String get modelHint => '接口使用的模型名称';
  @override
  String get visionModel => '看图模型（可选）';
  @override
  String get visionModelHint => '留空则用上面的模型看图';
  @override
  String get apiKey => 'API 密钥';
  @override
  String get apiKeyStored => '已保存密钥。输入新的会替换它。';
  @override
  String get apiKeyNotNeeded => '本机模型服务不需要';
  @override
  String get forgetKey => '删除已保存的密钥';
  @override
  String get keyPrivacy =>
      '密钥保存在资料库文件夹里，只有你的用户账户能读取，也不会再显示到这个窗口。选择云端运行时，关键帧截图会发送给这个接口；视频文件本身不会。';
  @override
  String get testConnection => '测试连接';
  @override
  String get saved => '已保存';
  @override
  String get endpointAnswered => '接口有响应';
  @override
  String get endpointNoList => '它没有列出模型，有些服务不提供这个列表。';
  @override
  String endpointHasModel(int n) => '你选的模型在它提供的 $n 个模型里。';
  @override
  String endpointLacksModel(int n, String sample) => '它提供 $n 个模型，但没有你选的那个：$sample';
  @override
  String get endpointSilent => '接口没有响应';
  @override
  String get noReason => '没有给出原因。';
  @override
  String get library => '资料库';
  @override
  String get librarySubtitle => '项目、分析缓存、视频管家和模型，所有项目共用';
  @override
  String get showInFinder => '在访达中显示';

  @override
  String get albumUnreadable => '无法读取视频管家';
  @override
  String get albumNameHint => '点一个人，查看他出现过的所有项目和视频，也可以在那里起名字；名字之后的项目里也会沿用。';
  @override
  String get albumEmpty => '还没有人物。运行过项目之后，人脸会出现在这里。';
  @override
  String get whoIsThis => '这是谁？';
  @override
  String get aQuickRun => '快速剪辑';
  @override
  String shots(int n) => '$n 个镜头';
  @override
  String get notAPerson => '不是人物';
  @override
  String get importModels => '从文件夹导入模型';
  @override
  String get importModelsNote => '没有网络时：先把离线模型包拷到这台设备上，再在这里选中那个文件夹。每个文件都会核对大小，电脑上还会核对校验值。';
  @override
  String get chooseFolder => '选择文件夹';
  @override
  String get importPicking => '正在读取文件夹…';
  @override
  String importDone(int imported, int already) =>
      '导入了 $imported 个模型文件${already > 0 ? '，另有 $already 个已经装好' : ''}';
  @override
  String get importNothing => '这个文件夹里没有认得的模型文件';
  @override
  String importFailed(String files) => '这些文件和模型目录对不上，没有导入：$files';
  @override
  String importTooBig(String models) => '$models 需要的内存比这台设备多，没有导入';
  @override
  String get captionsAndDance => '字幕与舞蹈';
  @override
  String get captionsAndDanceSubtitle => '画面描述模型按画面内容写字幕；姿态模型挑出跳舞的画面并衔接动作。默认用这台设备跑得动的最大一档，点「使用」可以换。';
  @override
  String get roleLabels => '场景标签';
  @override
  String get labelsSubtitle => '没有 Apple Vision 的设备用它给每个镜头标出地点、人物和在做什么，比如“湖边、情侣、跳舞”。';
  @override
  String get tooBigForDevice => '这台设备内存不够';
  @override
  String get bundledModel => '随安装包提供';
  @override
  String get inUse => '使用中';
  @override
  String get useModel => '使用';
  @override
  String subjectAppearsIn(int projects, int clips, int shots) =>
      '出现在 $projects 个项目、$clips 段视频里，共 $shots 个镜头';
  @override
  String clipsAndShots(int clips, int shots) => '$clips 段视频 · $shots 个镜头';
  @override
  String get openProject => '打开项目';
  @override
  String get clipNotRecorded => '没有记下是哪段视频（重新运行这个项目就会补上）';
  @override
  String get forgetSubjectTitle => '从人物里移除？';
  @override
  String get forgetSubjectBody => '只是整理人物分组，不会删除任何视频或项目。重新整理或重新运行项目时可能会再出现。';
  @override
  String get unnamed => '未命名';
  @override
  String clips(int n) => '$n 个视频';

  @override
  String get reloadEngine => '重新加载引擎状态';
  @override
  String get serveHint => '如果是网页版，请确认终端里的 `vb serve` 还在运行。';
  @override
  String get engine => '引擎';
  @override
  String get cannotAnalyse => '无法分析';
  @override
  String get analysisUnavailable => '无法分析视频';
  @override
  String get perceptionAdapters => '识别模块';
  @override
  String adapterChip(String role, String name) {
    const roles = {
      'Speech': '语音',
      'Vision': '画面',
      'Tracking': '人物',
      'Framing': '构图',
      'Composition': '构图评分',
      'Reasoning': '剪辑决策',
    };
    return '${roles[role] ?? role}：$name';
  }

  @override
  String get mockNote => '标为试验的模块还不产生真实结果，所以结果只是结构完整的草稿，不是成片。';
  @override
  String sourceVideos(int n) => '素材视频';
  @override
  String get choose => '选择';
  @override
  String get editingStyle => '剪辑风格';
  @override
  String get chosenFromFootage => '根据素材内容自动选择';
  @override
  String get template => '模板';
  @override
  String target(String duration) => '目标 $duration';
  @override
  String clipsRange(String from, String to) => '片段 $from–$to';
  @override
  String get subtitles => '字幕';
  @override
  String get analysing => '正在分析素材…';
  @override
  String get createADraft => '生成草稿';
  @override
  String get theRunFailed => '运行失败';

  @override
  String get hostingCta => '一键托管';
  @override
  String get hostingIntro =>
      '托管后，视频管家会读取本机的所有视频：按人脸、按画面内容分类，找出旅行、成长、年度和人群回忆，并在后台给每段回忆生成视频项目。全程在本机运行，不联网，不改动任何视频。';
  @override
  String get hostingStop => '停止托管';
  @override
  String get hostingAgain => '重新整理';
  @override
  String get hostingRunning => '正在整理';
  @override
  String hostingStatus(int videos, int photos, String? when, int memories) =>
      '已整理 ${footageCount(videos, photos)}，找到 $memories 段回忆${when == null ? '' : ' · 上次整理 $when'}';
  @override
  String get hostingNever => '还没有整理过。点「一键托管」开始。';
  @override
  String hostingStep(String step, String? file, int index, int count) => switch (step) {
        'hosting.find' => '查找视频和照片',
        'hosting.look' => '查看 ${index + 1}/$count${file == null ? '' : ' · $file'}',
        'hosting.memories' => '寻找回忆',
        'hosting.project' => '生成回忆视频 ${index + 1}/$count${file == null ? '' : '：$file'}',
        _ => '整理中',
      };
  @override
  String get hostingFolders => '整理的文件夹';
  @override
  String get hostingFoldersDefault => '本机常用的视频文件夹（影片、图片、桌面、文稿、下载）';
  @override
  String get changeFolders => '更改文件夹';
  @override
  String get addFolder => '添加文件夹';
  @override
  String get useDefaultFolders => '恢复默认';
  @override
  String get disksPluggedIn => '接着的磁盘（点一下整盘加入）';
  @override
  String get foldersUnreachable => '有的文件夹现在读不到';
  @override
  String get tabCategories => '分类';
  @override
  String get tabMemoryMap => '回忆地图';
  @override
  String get sectionFaces => '人脸';
  @override
  String get facesSubtitle => '按长相分组，不是人脸识别；点开查看 TA 是主角的所有视频，也可以起名字';
  @override
  String get facesEmpty => '还没有找到人脸。';
  @override
  String get sectionSmart => '智能分类';
  @override
  String get smartSubtitle => '按画面内容自动归类，点开预览、选择视频开始新项目';
  @override
  String get smartEmpty => '还没有归类的视频。';
  @override
  String videoCount(int n) => '$n 个视频';
  @override
  String footageCount(int videos, int photos) => photos == 0
      ? videoCount(videos)
      : videos == 0
          ? '$photos 张照片'
          : '$videos 个视频 · $photos 张照片';
  @override
  String get unverifiedBadge => '规则未经实测';
  @override
  String get removedCategories => '已删除的分类';
  @override
  String get restore => '恢复';
  @override
  String get removeCategory => '删除分类';
  @override
  String removeCategoryTitle(String name) => '删除「$name」分类？';
  @override
  String get removeCategoryBody =>
      '以后不再按这个分类整理视频，现有的归类也会去掉。视频本身不受影响，随时可以在视频管家底部恢复。';
  @override
  String get selectAll => '全选';
  @override
  String get selectNone => '取消全选';
  @override
  String selectedCount(int n) => '已选 $n 个';
  @override
  String newProjectFrom(int videos, [int photos = 0]) => photos == 0
      ? '用这 $videos 个视频新建项目'
      : videos == 0
          ? '用这 $photos 张照片新建项目'
          : '用这 $videos 个视频和 $photos 张照片新建项目';
  @override
  String get gridEmpty => '这里还没有视频。';
  @override
  String get fileTimeMark => '文件日期';
  @override
  String get hostedVideos => '视频管家整理的视频';
  @override
  String get seeInButler => '在视频管家中查看';
  @override
  String get mapAllMemories => '全部回忆';
  @override
  String memoryKindLabel(MemoryKindId kind) => switch (kind) {
        MemoryKindId.travel => '旅行记忆',
        MemoryKindId.growth => '成长记忆',
        MemoryKindId.annual => '年度记忆',
        MemoryKindId.group => '人群记忆',
      };
  @override
  String get mapEmpty => '还没有回忆。托管之后，旅行、成长、年度和人群回忆会钉在地图上。';
  @override
  String unpinnedMemories(int n) => '$n 段回忆没有定位，不在地图上';
  @override
  String memoryStatus(String? status, bool run) => switch (status) {
        'pending' => '等待生成视频',
        'failed' => '视频生成失败',
        'dismissed' => '项目已被删除',
        _ when run => '视频已生成',
        _ => '还没有生成视频',
      };
  @override
  String get generateProject => '生成视频';
  @override
  String get regenerate => '重新生成';
  @override
  String get orderLabel => '排列顺序';
  @override
  String get orderTime => '按时间';
  @override
  String get orderCategory => '按类别：人物→风景→食物→其他';
  @override
  String memoryDates(String start, String end) =>
      start == end ? start.replaceAll('-', '.') : '${start.replaceAll('-', '.')} – ${end.replaceAll('-', '.')}';
  @override
  String get memoryProjectNote => '回忆视频最多取 40 个片段，平均分布在这段时间里；年度回忆按类别比例取。';
  @override
  String get mapAttribution => '地图：Natural Earth（公有领域）';
  @override
  String memoriesHere(int n) => '这里有 $n 段回忆';
  @override
  String get mapLayers => '图层';
  @override
  String mapStats(int memories, int places, int regions) =>
      '$memories 段回忆 · $places 处地点 · $regions 个地区';
  @override
  String get mapResetView => '看全部';
  @override
  String get memoryGenerating => '正在生成，完成后会出现在项目里';
  @override
  String get mergePerson => '和另一个人合并';
  @override
  String get butlerNotesTitle => '这些分类是怎么来的';
  @override
  String get memoryBadge => '视频管家生成的回忆';
  @override
  String get mergeTitle => '是同一个人吗？';
  @override
  String get mergeBody => '戴口罩墨镜、或者小时候和长大后的脸，常被分成两组。选另一组，合成同一个人；名字保留已起的那个。';

  // --- the built-in editor (0.2.0) -------------------------------------------
  @override
  String get edCardTitle => '成片';
  @override
  String get edCardBody => '在软件内继续剪辑；剪好后导出视频，或者导出剪映、CapCut 工程文件继续修改。';
  @override
  String get edEditInApp => '在软件内剪辑';
  @override
  String get edExportProject => '导出';
  @override
  String edEditedOn(String date) => '上次编辑 $date';
  @override
  String get edExportsTitle => '导出记录';
  @override
  String edExportKind(String kind) => switch (kind) {
        'video' => '视频',
        'jianying' => '剪映草稿',
        'capcut' => 'CapCut 草稿',
        'otio' => 'OTIO 时间线',
        'clips' => '分段视频',
        _ => kind,
      };
  @override
  String get edOpening => '正在打开剪辑…';
  @override
  String edEdited(int percent) => '已修改 · 改动了 $percent% 的素材';
  @override
  String get edAsMade => 'AI 成片，尚未修改';
  @override
  String get edUndo => '撤销';
  @override
  String get edRedo => '重做';
  @override
  String get edExport => '导出';
  @override
  String get edTitle => '标题';
  @override
  String get edRevertAi => '恢复 AI 成片';
  @override
  String get edRevertAiBody => '回到 AI 剪出的版本。这一步也可以撤销。';
  @override
  String get edRestartFromRun => '用最新成片重新开始';
  @override
  String get edRestartBody => '放弃现在的修改，从项目最近一次生成的成片重新开始。现在的版本会另存在项目文件夹里（cut.previous）。';
  @override
  String get edNewerRun => '这个项目重新生成过成片，你正在编辑的是之前的版本。';
  @override
  String get edPlay => '播放（空格）';
  @override
  String get edPause => '暂停（空格）';
  @override
  String get edPrevFrame => '上一帧';
  @override
  String get edNextFrame => '下一帧';
  @override
  String get edZoomIn => '放大时间线';
  @override
  String get edZoomOut => '缩小时间线';
  @override
  String get edBack => '返回';
  @override
  String get edSplit => '分割';
  @override
  String get edDelete => '删除';
  @override
  String get edSpeed => '变速';
  @override
  String get edVolume => '音量';
  @override
  String get edDuration => '时长';
  @override
  String get edAdjust => '调节';
  @override
  String get edPosition => '位置';
  @override
  String get edBackground => '背景';
  @override
  String get edBackgroundAll => '背景（全部片段）';
  @override
  String get edDuplicate => '复制';
  @override
  String get edMoveEarlier => '前移';
  @override
  String get edMoveLater => '后移';
  @override
  String get edTransition => '转场';
  @override
  String get edTransitionAll => '全部转场';
  @override
  String get edEditText => '编辑';
  @override
  String get edTextToPlayhead => '移到这里';
  @override
  String get edReplaceMusic => '替换';
  @override
  String get edAddClip => '添加片段';
  @override
  String get edText => '文字';
  @override
  String get edMusic => '音乐';
  @override
  String get edCanvas => '画布';
  @override
  String get edSoundOff => '关闭原声';
  @override
  String get edSoundOn => '开启原声';
  @override
  String get edNewText => '文字';
  @override
  String get edSpeedNote => '片段按这个倍数播放，声音保持原来的音调。';
  @override
  String get edMute => '静音';
  @override
  String get edMuteAll => '关闭全部片段原声';
  @override
  String get edTransCut => '无';
  @override
  String get edTransXfade => '叠化';
  @override
  String get edTransFade => '闪黑';
  @override
  String get edTransWipeLeft => '向左擦除';
  @override
  String get edTransWipeRight => '向右擦除';
  @override
  String get edLength => '时长';
  @override
  String get edApplyToAll => '应用到全部';
  @override
  String get edReset => '重置';
  @override
  String get edBrightness => '亮度';
  @override
  String get edContrast => '对比度';
  @override
  String get edSaturation => '饱和度';
  @override
  String get edScale => '缩放';
  @override
  String get edMoveX => '左右';
  @override
  String get edMoveY => '上下';
  @override
  String get edRotate => '旋转';
  @override
  String get edRotate90 => '旋转 90°';
  @override
  String get edRatio => '比例';
  @override
  String get edRatioOriginal => '原始';
  @override
  String get edBgBlack => '黑色';
  @override
  String get edBgBlur => '模糊';
  @override
  String get edTextHint => '输入文字';
  @override
  String get edDone => '完成';
  @override
  String get edFontSize => '字号';
  @override
  String get edBold => '粗体';
  @override
  String get edOutline => '描边';
  @override
  String get edTextBox => '底色';
  @override
  String get edPlace => '位置';
  @override
  String get edPlaceTop => '顶部';
  @override
  String get edPlaceMiddle => '居中';
  @override
  String get edPlaceBottom => '底部';
  @override
  String get edDragTextHint => '也可以在预览画面里直接拖动文字。';
  @override
  String get edFadeIn => '淡入';
  @override
  String get edFadeOut => '淡出';
  @override
  String get edRemoveMusic => '删除音乐';
  @override
  String get edFootageNote => '从这个项目分析过的素材里选一段，插入到播放头所在片段之后。';
  @override
  String edUsedTimes(int n) => '成片里已用了 $n 段';
  @override
  String get edNoStretches => '没有可用的片段';
  @override
  String get edExportVideo => '导出视频';
  @override
  String get edLosslessTitle => '无损快速导出';
  @override
  String get edLosslessChecking => '正在检查能否无损导出…';
  @override
  String get edLosslessButton => '无损导出（不重新编码）';
  @override
  String edLosslessNote(double extraSeconds) => extraSeconds < 0.05
      ? '直接拷贝原片的数据：画质和原片完全一样，几秒就能导完。'
      : '直接拷贝原片的数据：画质和原片完全一样，几秒就能导完。每段从它前面最近的关键帧开始，到画面完整处结束，一共多出 ${extraSeconds.toStringAsFixed(1)} 秒。';
  @override
  String get edLosslessBlocked => '这一版要重新编码才能导出，因为：';
  @override
  String get edClipsButton => '分段导出（每段一个文件）';
  @override
  String get edClipsNote => '没改过画面的片段直接拷贝原片，其余逐段渲染；文字、转场和配乐属于整条成片，不在单个片段里。';
  @override
  String edClipsDone(int copied, int rendered) => '直接拷贝 $copied 段，重新渲染 $rendered 段';
  @override
  String get edNoVideoExport => '这台设备上还不能导出视频（这个版本没有视频编码器），可以导出下面的工程文件。';
  @override
  String get edResolution => '分辨率';
  @override
  String get edFrameRate => '帧率';
  @override
  String get edFrameRateSource => '跟随素材';
  @override
  String get edCodec => '编码';
  @override
  String get edCodecH264 => 'H.264（兼容最好）';
  @override
  String get edCodecHevc => 'HEVC（文件更小）';
  @override
  String get edExportDrafts => '导出工程文件';
  @override
  String get edDraftNote => '工程文件保留每一刀，可以在别的剪辑软件里继续修改。';
  @override
  String get edDraftJianying => '剪映草稿';
  @override
  String get edDraftCapcut => 'CapCut 草稿';
  @override
  String get edDraftOtio => 'OTIO 时间线';
  @override
  String get edDraftOtioNote => 'DaVinci Resolve、Premiere 等可以导入';
  @override
  String edEditorMissing(String name) => '没有找到$name的草稿文件夹，先安装并打开一次$name';
  @override
  String get edExporting => '正在导出视频…';
  @override
  String edFrames(int done, int total) => '第 $done / $total 帧';
  @override
  String get edThermal => '设备较热，导出放慢了一些。';
  @override
  String get edExported => '导出完成';
  @override
  String get edExportedTo => '位置';
  @override
  String get edShowInFolder => '在文件夹中显示';
  @override
  String edExportFailed(String error) => '导出失败：$error';
}

class SEn extends S {
  const SEn();

  @override
  AppLanguage get language => AppLanguage.en;

  @override
  String get cacheTitle => 'Earlier analysis';
  @override
  String cacheSubtitle(int entries, String size) =>
      'What earlier runs found is kept ($entries entries, $size), so the same videos are not '
      'analysed twice.';
  @override
  String get cacheEmpty => 'Nothing has been analysed yet.';
  @override
  String get clearCache => 'Clear all analysis';
  @override
  String get clearCacheTitle => 'Clear all analysis?';
  @override
  String get clearCacheBody =>
      'Every video will be analysed from the start next time. Projects, drafts already written, '
      'the album and downloaded models are kept.';
  @override
  String cacheCleared(String size) => 'Cleared $size';
  @override
  String get draftMediaTitle => 'How drafts reach your footage';
  @override
  String get draftMediaSubtitle =>
      '剪映 and CapCut on a Mac may read only the Movies folder and files you picked inside them';
  @override
  String get draftMediaLink => 'Hard link (recommended)';
  @override
  String get draftMediaLinkNote =>
      'A hard link to the original goes in the draft\'s Resources folder: the same data under a '
      'second name, no copy and no extra space (Finder counts its size twice), and the editor '
      'opens it directly.';
  @override
  String get draftMediaReference => 'Original paths only';
  @override
  String get draftMediaReferenceNote =>
      'The draft names the original files only. Footage outside Movies opens marked as not '
      'readable, and has to be relinked in the editor.';


  @override
  String get appName => 'Video Butler';
  @override
  String get cancel => 'Cancel';
  @override
  String get save => 'Save';
  @override
  String get delete => 'Delete';
  @override
  String get keepIt => 'Keep it';
  @override
  String get remove => 'Remove';
  @override
  String get change => 'Change';
  @override
  String get automatic => 'Automatic';
  @override
  String get ready => 'Ready';
  @override
  String get name => 'Name';
  @override
  String get installed => 'Installed';
  @override
  String andMore(int n) => 'and $n more';
  @override
  String copied(String label) => 'Copied $label';
  @override
  String get copyPath => 'Copy path';
  @override
  String couldNotSave(String error) => 'Could not save: $error';

  @override
  String get navProjects => 'Projects';
  @override
  String get navAlbum => 'Butler';
  @override
  String get navModels => 'Models';
  @override
  String get navSettings => 'Settings';
  @override
  String get navQuickDraft => 'Quick draft';

  @override
  String get selfTestIntro => 'Checking this machine before the first run.';
  @override
  String get engineUnreachable => 'The engine could not be reached';
  @override
  String get environment => 'Environment';
  @override
  String get envAllGood => 'Everything is in place';
  @override
  String get envMissing => 'Something needed for a run is missing';
  @override
  String get envWorthKnowing => 'Ready, with some things worth knowing';
  @override
  String get cannotRun => 'Cannot run';
  @override
  String get checkAgain => 'Check again';
  @override
  String get continueLabel => 'Continue';

  @override
  String get projectsUnreadable => 'Projects could not be read';
  @override
  String get peopleInFootage => 'People in your footage';
  @override
  String get peopleEmptyHint =>
      'Faces from earlier projects appear here, grouped by how alike they look';
  @override
  String peopleFound(int n) => '$n found across your projects';
  @override
  String get openAlbum => 'Open the butler';
  @override
  String get nobodyYet => 'Nobody yet.';
  @override
  String get projects => 'Projects';
  @override
  String get newProject => 'New project';
  @override
  String get noProjects =>
      'No projects yet. A project is some footage, a style, and where the draft should go.';
  @override
  String get projectNameHint => 'Kyoto in spring';
  @override
  String get create => 'Create';
  @override
  String items(int n) => '$n ${n == 1 ? 'item' : 'items'}';
  @override
  String get onTheBeat => 'On the beat';
  @override
  String get cloud => 'Cloud';
  @override
  String lastRunOn(String date) => 'last run $date';

  @override
  String get rename => 'Rename';
  @override
  String get renameProject => 'Rename project';
  @override
  String get showProjectFolder => 'Show the project folder';
  @override
  String get deleteProject => 'Delete project';
  @override
  String deleteProjectTitle(String name) => 'Delete "$name"?';
  @override
  String get deleteProjectBody =>
      'This removes the project and the drafts it wrote into its own folder. Your footage is not '
      'touched, drafts already in your editor stay there, and analysis other projects share is kept.';
  @override
  String get stoppedKept =>
      'Stopped. Clips analysed so far are kept, so running again picks up from there.';
  @override
  String get runFailed => 'The run failed.';
  @override
  String get noDraftThisTime => 'No draft this time';
  @override
  String get footage => 'Footage';
  @override
  String get footageEmptySubtitle =>
      'Several videos and photos, or whole folders. Folders are read with everything inside them.';
  @override
  String itemsChosen(int n) => '$n ${n == 1 ? 'item' : 'items'} chosen';
  @override
  String get addFootage => 'Add videos, photos or a folder';
  @override
  String get noVideosYet => 'Nothing chosen yet. Videos and photos both work: pick several, or a whole folder.';
  @override
  String get style => 'Style';
  @override
  String get styleSubtitle => 'Mood sets the pacing, the topic sets the structure';
  @override
  String get mood => 'Mood';
  @override
  String get topic => 'Topic';
  @override
  String moodLabel(String id) =>
      const {'happy': 'Happy', 'sad': 'Sad', 'humor': 'Humorous'}[id] ?? id;
  @override
  String topicLabel(String id) =>
      const {
        'dance': 'Dance',
        'travel': 'Travel',
        'growth': 'Growing up',
        'continuity': 'Subject continuity',
        'annual': 'A year in review',
        'together': 'Together',
      }[id] ??
      id;
  @override
  String get cutOnTheBeat => 'Cut on the beat';
  @override
  String get beatWithMusic => 'Every cut lands on a beat of the music.';
  @override
  String get beatWithoutMusic =>
      'Every cut lands on a beat: of the footage\'s own music when it has a clear one, of a '
      'steady tempo otherwise.';
  @override
  String get length => 'Length';
  @override
  String get lengthSubtitle => 'How long the finished piece should be';
  @override
  String durationChoice(int seconds) =>
      seconds < 60 ? '$seconds s' : '${seconds ~/ 60} min';
  @override
  String get lengthNote =>
      'Automatic lets the style decide, about 25 seconds to a minute. With too little footage '
      'the piece comes out shorter.';
  @override
  String get lengthNoteContinuity =>
      'Automatic is the whole action\'s own length. On the beat, the steps are put on the beat '
      'first (at most 1.2x faster or slower), and a shorter chosen length shortens every clip '
      'alike; off the beat, every clip is sped up alike first (at most 1.2x), then shortened.';
  @override
  String get continuityHint =>
      'One action, put together from every clip that shows it: at each cut the person keeps their '
      'place, size and movement, and only the scene changes. On the beat, the dance\'s own beat is '
      'read first; wherever someone stands still for more than a beat and a half, or watches a '
      'phone (learning the steps), is left out, and a take mostly spent not dancing is left out '
      'whole. Pictures are enlarged, moved and turned slightly to match, all of it adjustable in '
      'the editor. People only for now; needs the RTMO-s pose model.';
  @override
  String get musicLoopContinuity =>
      'When the music is shorter than the action: on, it plays again from its first beat; off, '
      'the rest of the cut is silent. The action is never sped up to fit the music.';
  @override
  String get music => 'Music';
  @override
  String get musicSubtitle =>
      'Laid under the cut; on the beat, every cut lands on one of its beats';
  @override
  String get noMusic => 'No music. The draft keeps the footage\'s own sound.';
  @override
  String get importMusic => 'Import music';
  @override
  String musicFacts(String duration, double? bpm, bool steady) => bpm != null && steady
      ? '$duration · ${bpm.toStringAsFixed(0)} BPM'
      : '$duration · no clear beat';
  @override
  String get musicReading => 'Listening for the beat…';
  @override
  String get models => 'Models';
  @override
  String get modelsSubtitle =>
      'Local runs entirely on this device. Cloud sends stills to an endpoint you set up.';
  @override
  String get local => 'Local';
  @override
  String get cloudApi => 'Cloud (API)';
  @override
  String get roleSpeech => 'Speech';
  @override
  String get roleVision => 'Vision';
  @override
  String get roleFraming => 'Framing';
  @override
  String get notDownloadable => 'Not yet downloadable';
  @override
  String get framingSubtitle =>
      'Three small models read each shot together: depth finds what is right against the '
      'lens, and faces and people tell a person from an obstruction. All on this device.';
  @override
  String get noFramingModel =>
      'No framing models yet, so shots blocked by a door or a dashboard are not caught.';
  @override
  String get framingOnThisMac => 'Depth, faces and people, on this device';
  @override
  String get noSpeechModel =>
      'No speech model. Captions are written from the picture and do not need one; only '
      'interview styles turn speech into subtitles.';
  @override
  String speechOnThisMac(String model) => '$model, on this device';
  @override
  String get roleCaptions => 'Captions';
  @override
  String captionsOnThisMac(String model) =>
      '$model writes captions from the picture, on this device';
  @override
  String get noCaptionModel =>
      'No picture-description model, so no captions (speech is never turned into them)';
  @override
  String get rolePose => 'Pose';
  @override
  String poseOnThisMac(String model) =>
      '$model: a dance cut takes only dancing, and joins the movement';
  @override
  String get noPoseModel => 'No pose model: a dance cut can only go by how much the picture moves';
  @override
  String get getAModel => 'Get a model';
  @override
  String visionOverEndpoint(String model, String url) => '$model at $url';
  @override
  String get visionNone => 'Nothing looks at the footage in this build.';
  @override
  String get visionApple => 'Apple Vision on this Mac: scene labels and faces';
  @override
  String get visionSiglip => 'SigLIP 2 on this device: scene labels (no face grouping here)';
  @override
  String get noEndpoint => 'No endpoint is set up';
  @override
  String get noEndpointBody =>
      'Until one is, a cloud run does everything on this device and says so.';
  @override
  String get cloudSettings => 'Cloud endpoint settings';
  @override
  String get whereDraftGoes => 'Where the draft goes';
  @override
  String get whereDraftGoesSubtitle => 'The engine decides the cuts; your editor does the cutting';
  @override
  String get editorNotInstalled => 'Not installed on this device';
  @override
  String editorOnPhone(String name) =>
      'Installed on this device, but the phone app keeps its drafts in its own storage, which '
      'Android lets no other app write to, so a draft cannot be made for it here.';
  @override
  String handOffTitle(String name) => 'Continue in $name';
  @override
  String handOffBody(int clips, String name) =>
      'Hands these $clips clips to $name, cut and in order, and it opens them as a new project. '
      'Go back to $name\'s home screen first: if a project is open there, the clips are added '
      'to it.';
  @override
  String handOffLeftOut(int titleCards, int texts, int transitions) {
    final parts = [
      if (titleCards > 0) '$titleCards title card${titleCards == 1 ? '' : 's'}',
      if (texts > 0) '$texts text${texts == 1 ? '' : 's'}',
      if (transitions > 0) '$transitions transition${transitions == 1 ? '' : 's'}',
    ];
    return parts.isEmpty ? '' : 'Left behind: ${parts.join(', ')}.';
  }

  @override
  String handOffMusic(String music, String start) =>
      'Add the music in the editor: $music, from $start, so the cuts land on the beat.';
  @override
  String handOffButton(String name) => 'Open in $name';
  @override
  String handOffCutting(int done, int total) => 'Cutting clip $done of $total';
  @override
  String handOffDone(String name, String folder) =>
      'Handed to $name. The cut clips are in $folder; $name\'s project uses them, so keep them '
      'until you are done.';
  @override
  String handOffFailed(String name, String why) => 'Could not hand it to $name: $why';
  @override
  String get editorNoFolder =>
      'Installed, but it has not created its draft folder yet. Open it once.';
  @override
  String get editorRunning => 'Open now, so a new draft appears after you quit and reopen it';
  @override
  String get editorReady => 'A draft appears in its project list, ready to edit';
  @override
  String get otioDraft => 'OpenTimelineIO draft';
  @override
  String get otioAlways => 'Always written to the project folder, for other editors';
  @override
  String get createDraft => 'Create the draft';
  @override
  String createDraftFor(String editor) => 'Create the draft and send it to $editor';
  @override
  String get lastRun => 'Last run';
  @override
  String get durationStat => 'Duration';
  @override
  String get clipsStat => 'Clips';
  @override
  String get draftLabel => 'Draft';
  @override
  String get editorDraftLabel => 'Editor draft';

  @override
  String get starting => 'Starting';
  @override
  String stageLabel(String id, String fallback) => fallback.isEmpty ? id : fallback;
  @override
  String clipOf(int index, int count) => 'Clip $index of $count';
  @override
  String clipsAnalysing(int done, int count, int active) =>
      '$done of $count clips done${active > 1 ? ' · $active at once' : ''}';

  @override
  String get analysisReuse => 'Reuse earlier analysis';
  @override
  String get analysisFresh => 'Analyse again';
  @override
  String get analysisChoiceNote =>
      'Reuse: clips analysed before are not analysed again; only new ones are. Analyse again: '
      'every clip is looked at from the start, which takes longer.';
  @override
  String get musicShortTitle => 'The music is shorter than the cut';
  @override
  String musicShortBody(String music, String cut) =>
      'The music runs $music and the cut up to $cut, so the end of the cut would be silent. Play '
      'the song again?\nIt picks up from the first beat, so the cuts stay on the beat. The '
      'project remembers the answer; change it under Music.';
  @override
  String get loopMusic => 'Play it again';
  @override
  String get dontLoopMusic => 'No, leave the end silent';
  @override
  String get musicLoopSwitch => 'Repeat the music when the cut runs longer';
  @override
  String get musicLoopSwitchNote => 'From the first beat, so the cuts stay on the beat';
  @override
  String stepLabel(String step, String? file, String fallback) => fallback;
  @override
  String get stop => 'Stop';
  @override
  String get stopping => 'Stopping...';

  @override
  String get roughStructure => 'This is a rough structure, not a finished cut';
  @override
  String get readyInEditor => 'Ready in your editor';
  @override
  String get readyInEditorSubtitle =>
      'The cut decisions are in the draft. The editor does the cutting, and every clip, trim '
      'and title can still be changed there.';
  @override
  String draftFacts(int clips, bool music, int transitionsDropped, int linked, int unreadable) => [
        '$clips clips',
        if (music) 'with the music',
        if (transitionsDropped > 0) '$transitionsDropped transitions as hard cuts',
        if (linked > 0)
          '$linked files hard-linked into the draft (the same files, no extra space) so the '
              'editor may read them',
        if (unreadable > 0) '$unreadable files referenced where they are; the editor may ask to relink them',
      ].join(', ');
  @override
  String get showDraftFolder => 'Show the draft folder';
  @override
  String openEditor(String editor) => 'Open $editor';
  @override
  String get restartEditorNote =>
      'If the editor was already open, quit and reopen it: it reads its draft folder when it '
      'starts.';
  @override
  String get howItWasMade => 'How this was made';
  @override
  String get result => 'Result';
  @override
  String styleName(String templateId, String fallback) =>
      fallback.isEmpty ? templateId : fallback;
  @override
  String cutOnBeat(String bpmText) => 'cut on $bpmText';
  @override
  String beatDescription(String origin, double bpm) => switch (origin) {
        'music' => '${bpm.toStringAsFixed(0)} BPM from the music',
        'source' => '${bpm.toStringAsFixed(0)} BPM from the footage',
        _ => 'a steady ${bpm.toStringAsFixed(0)} BPM, as no music was chosen',
      };
  @override
  String get reusedAnalysis => 'Reused analysis';
  @override
  String reusedOf(int x, int y) => 'Reused $x of $y';
  @override
  String get shotsFound => 'Shots found';
  @override
  String get clipsUsed => 'Clips used';
  @override
  String get videos => 'Videos';
  @override
  String get peopleFoundStat => 'People found';
  @override
  String get grounded => 'Grounded';
  @override
  String get rejected => 'Rejected';
  @override
  String get theCut => 'The cut';
  @override
  String get theCutSubtitle => 'In order, with the reason each clip was chosen';
  @override
  String get titleCard => 'Title card';
  @override
  String get everyShot => 'Every shot detected';
  @override
  String get everyShotSubtitle => 'Including the ones left out, so the selection can be judged';
  @override
  String get rejectedByGrounding => 'Rejected by grounding';
  @override
  String get rejectedByGroundingSubtitle =>
      'These decisions did not resolve to real footage, so they were dropped rather than '
      'corrected';
  @override
  String get filesWritten => 'Files written';
  @override
  String get filesWrittenSubtitle => 'The draft opens in any OpenTimelineIO-aware editor';
  @override
  String get footageGraph => 'Footage graph';
  @override
  String get editingDsl => 'Editing DSL';

  @override
  String get parentDirectory => 'Parent directory';
  @override
  String get chooseVideos => 'Choose videos and photos';
  @override
  String get chooseMusic => 'Choose music';
  @override
  String get useThisFolder => 'Use this folder';
  @override
  String videosChosen(int n) => n == 1 ? '1 video chosen' : '$n videos chosen';
  @override
  String get clear => 'Clear';
  @override
  String get useThese => 'Use these';
  @override
  String get noMusicHere => 'No folders or music here.\nUse the arrow above to go back up.';
  @override
  String get close => 'Close';
  @override
  String get thermalTitle => 'Device temperature';
  @override
  String get thermalSwitch => 'Slow down when the device gets hot';
  @override
  String get thermalNote =>
      'When the device warms up during a run, it slows down: rests between steps and analyses '
      'one clip at a time; when it is hot, it pauses until it has cooled. It goes by the '
      'system\'s own reading, ahead of the system closing the app for overheating. Off, it '
      'always runs at full speed.';
  @override
  String get thermalSlowing => 'The device is warm; running slower so it does not overheat';
  @override
  String get thermalCooling => 'The device is hot; pausing until it cools, then carrying on';
  @override
  String get previewClip => 'Play a preview';
  @override
  String get previewFailed => 'This clip cannot be played here. It can still be chosen and analysed.';
  @override
  String get chooseClip => 'Choose this clip';
  @override
  String get chosenClip => 'Chosen';
  @override
  String placeLabel(String id) =>
      const {
        'music': 'Music',
        'downloads': 'Downloads',
        'home': 'Home',
        'desktop': 'Desktop',
        'documents': 'Documents',
        'movies': 'Movies',
        'pictures': 'Pictures',
        'dcim': 'Camera',
        'netease': 'NetEase Cloud Music',
        'qqmusic': 'QQ Music',
        'kugou': 'Kugou',
        'kuwo': 'Kuwo',
      }[id] ??
      id;
  @override
  String get noMusicOnPhone =>
      'No usable songs here. If the phone has some, allow "Music and audio" in Settings > Apps > '
      'Video Butler > Permissions. Songs a music app downloaded encrypted for members (.ncm, .qmc, '
      '.mflac, .kgm and the like) play only in that app, and cannot be seen or used here.';
  @override
  String get noVideosHere =>
      'No folders, videos or photos here.\nUse the arrow above to go back up, or take the whole folder.';
  @override
  String get deselectAll => 'Deselect all';
  @override
  String shownOf(int shown, int total) =>
      shown == total ? '$total items' : '$shown of $total items';
  @override
  String get readingClips => 'Reading when and how each clip was filmed...';
  @override
  String get filterDate => 'Filmed on';
  @override
  String get filterRatio => 'Frame';
  @override
  String get filterKind => 'Kind';
  @override
  String kindOption(String kind, int n) => '${kind == 'image' ? 'Photos' : 'Videos'} · $n';
  @override
  String footageChosen(int videos, int photos) => '${[
        if (videos > 0) '$videos ${videos == 1 ? 'video' : 'videos'}',
        if (photos > 0) '$photos ${photos == 1 ? 'photo' : 'photos'}',
      ].join(' and ')} chosen';
  @override
  String get previewPhoto => 'View the photo';
  @override
  String get chooseThisPhoto => 'Choose this photo';
  @override
  String get importFromPhotos => 'Import from Photos';
  @override
  String get importFromPhotosNote =>
      'The videos and photos you choose are copied into this app to be analysed. Nothing is uploaded.';
  @override
  String get browseAppFiles => 'Browse the files in this app';
  @override
  String get allOption => 'All';
  @override
  String get unknownDate => 'Date unknown';
  static const _months = [
    'Jan',
    'Feb',
    'Mar',
    'Apr',
    'May',
    'Jun',
    'Jul',
    'Aug',
    'Sep',
    'Oct',
    'Nov',
    'Dec',
  ];
  @override
  String dayLabel(DateTime day) => '${_months[day.month - 1]} ${day.day}';
  @override
  String monthLabel(DateTime month) =>
      '${_months[month.month - 1]} ${month.year}';
  @override
  String orientationLabel(String orientation) =>
      const {
        'landscape': 'landscape',
        'portrait': 'portrait',
        'square': 'square'
      }[orientation] ??
      'unknown shape';
  @override
  String get fileDateNote => ' (file date)';
  @override
  String get noMatches => 'Nothing matches these filters.';

  @override
  String get modelsUnreadable => 'The model library could not be read';
  @override
  String get downloadedOnce => 'Downloaded once, used by every project';
  @override
  String modelsKeptIn(String dir) => 'Models are kept in $dir.';
  @override
  String get visionNeedsNoDownload =>
      'Scene labels and faces need no download on a Mac: Apple Vision is built in. Captions '
      'come from the picture-description model; a speech model only transcribes interviews.';
  @override
  String get visionOnPhone =>
      'Scene labels come from SigLIP 2 on this device, which the app comes with; face grouping '
      'comes from Apple Vision, which only a Mac has. Captions come from the picture-description '
      'model; a speech model only transcribes interviews.';
  @override
  String get speechSubtitle => 'whisper.cpp, multilingual, including Chinese';
  @override
  String get download => 'Download';
  @override
  String deleteModelTitle(String name) => 'Delete $name?';
  @override
  String deleteModelBody(String size) =>
      'It frees $size. Every project stops using it until it is downloaded again.';
  @override
  String downloadFailed(String error) => 'Download failed: $error';

  @override
  String get settingsUnreadable => 'Settings could not be read';
  @override
  String get languageTitle => 'Language';
  @override
  String get cloudModel => 'Cloud model';
  @override
  String get cloudModelSubtitle =>
      'Any OpenAI-compatible endpoint: a hosted API, or a model server on this computer such as '
      'http://localhost:11434/v1';
  @override
  String get setUp => 'Set up';
  @override
  String get notSetUp => 'Not set up';
  @override
  String get baseUrl => 'Base URL';
  @override
  String get model => 'Model';
  @override
  String get modelHint => 'The model name the endpoint expects';
  @override
  String get visionModel => 'Vision model (optional)';
  @override
  String get visionModelHint => 'Leave empty to use the model above for stills too';
  @override
  String get apiKey => 'API key';
  @override
  String get apiKeyStored => 'A key is stored. Type a new one to replace it.';
  @override
  String get apiKeyNotNeeded => 'Not needed for a local model server';
  @override
  String get forgetKey => 'Forget the stored key';
  @override
  String get keyPrivacy =>
      'The key is kept in the library folder, readable only by your user account, and is never '
      'sent back to this window. When a run uses the cloud, its keyframe stills are sent to this '
      'endpoint; your video files are not.';
  @override
  String get testConnection => 'Test connection';
  @override
  String get saved => 'Saved';
  @override
  String get endpointAnswered => 'The endpoint answered';
  @override
  String get endpointNoList => 'It did not list its models, which some servers do not.';
  @override
  String endpointHasModel(int n) => 'The model you chose is among the $n it offers.';
  @override
  String endpointLacksModel(int n, String sample) =>
      'It offers $n models, and the one you chose is not among them: $sample';
  @override
  String get endpointSilent => 'The endpoint did not answer';
  @override
  String get noReason => 'No reason was given.';
  @override
  String get library => 'Library';
  @override
  String get librarySubtitle =>
      'Projects, the analysis cache, the album and models, shared by every project';
  @override
  String get showInFinder => 'Show in Finder';

  @override
  String get albumUnreadable => 'The album could not be read';
  @override
  String get albumNameHint =>
      'Tap someone to see every project and clip they are in, and to name them. The name '
      'follows them into later projects.';
  @override
  String get albumEmpty => 'Nobody yet. Faces appear here after a project is run.';
  @override
  String get whoIsThis => 'Who is this?';
  @override
  String get aQuickRun => 'A quick run';
  @override
  String shots(int n) => '$n shots';
  @override
  String get notAPerson => 'Not a person';
  @override
  String get importModels => 'Import models from a folder';
  @override
  String get importModelsNote =>
      'With no network: copy the offline model pack onto this device, then choose its folder '
      'here. Every file is checked for size, and on a computer for its checksum too.';
  @override
  String get chooseFolder => 'Choose folder';
  @override
  String get importPicking => 'Reading the folder...';
  @override
  String importDone(int imported, int already) =>
      'Imported $imported model files${already > 0 ? '; $already were already installed' : ''}';
  @override
  String get importNothing => 'No model files this app knows are in that folder';
  @override
  String importFailed(String files) =>
      'These did not match the catalogue and were not imported: $files';
  @override
  String importTooBig(String models) =>
      '$models needs more memory than this device has, so it was not imported';
  @override
  String get captionsAndDance => 'Captions and dance';
  @override
  String get captionsAndDanceSubtitle =>
      'The picture-description model writes captions from the picture; the pose model finds '
      'the dancing and joins the movement. The largest one this device can run is used; Use '
      'switches to another.';
  @override
  String get roleLabels => 'Scene labels';
  @override
  String get labelsSubtitle =>
      'On a device without Apple Vision, this names each shot\'s place, people and activity, '
      'such as "lake, couple, dancing".';
  @override
  String get tooBigForDevice => 'Too large for this device';
  @override
  String get bundledModel => 'Came with the app';
  @override
  String get inUse => 'In use';
  @override
  String get useModel => 'Use';
  @override
  String subjectAppearsIn(int projects, int clips, int shots) =>
      'In $projects ${projects == 1 ? 'project' : 'projects'} and $clips '
      '${clips == 1 ? 'clip' : 'clips'}, $shots ${shots == 1 ? 'shot' : 'shots'} in all';
  @override
  String clipsAndShots(int clips, int shots) =>
      '$clips ${clips == 1 ? 'clip' : 'clips'} · $shots ${shots == 1 ? 'shot' : 'shots'}';
  @override
  String get openProject => 'Open project';
  @override
  String get clipNotRecorded => 'Which clip was not recorded (running the project again fills it in)';
  @override
  String get forgetSubjectTitle => 'Remove from the album?';
  @override
  String get forgetSubjectBody =>
      'This tidies the album; no clip or project is deleted. They may come back when a project '
      'is run again.';
  @override
  String get unnamed => 'Unnamed';
  @override
  String clips(int n) => '$n ${n == 1 ? 'clip' : 'clips'}';

  @override
  String get reloadEngine => 'Reload engine state';
  @override
  String get serveHint =>
      'If this is the web app, make sure `vb serve` is still running in a terminal.';
  @override
  String get engine => 'Engine';
  @override
  String get cannotAnalyse => 'Cannot analyse';
  @override
  String get analysisUnavailable => 'Video analysis is unavailable';
  @override
  String get perceptionAdapters => 'Perception adapters';
  @override
  String adapterChip(String role, String name) => '$role: $name';
  @override
  String get mockNote =>
      'The adapters marked as experiments produce no real output yet, so a result is a '
      'structurally complete draft rather than a finished cut.';
  @override
  String sourceVideos(int n) => n > 1 ? 'Source videos' : 'Source video';
  @override
  String get choose => 'Choose';
  @override
  String get editingStyle => 'Editing style';
  @override
  String get chosenFromFootage => 'Chosen automatically from what the footage contains';
  @override
  String get template => 'Template';
  @override
  String target(String duration) => 'target $duration';
  @override
  String clipsRange(String from, String to) => 'clips $from to $to';
  @override
  String get subtitles => 'subtitles';
  @override
  String get analysing => 'Analysing the footage...';
  @override
  String get createADraft => 'Create a draft';
  @override
  String get theRunFailed => 'The run failed';

  @override
  String get hostingCta => 'Hand it all over';
  @override
  String get hostingIntro =>
      'The butler reads every video on this device, files each by face and by what it shows, '
      'finds the trips, the people growing up, the years and the groups in them, and cuts a '
      'project for each memory in the background. All on this device: no network, and no video '
      'is changed.';
  @override
  String get hostingStop => 'Stop';
  @override
  String get hostingAgain => 'Look again';
  @override
  String get hostingRunning => 'Sorting';
  @override
  String hostingStatus(int videos, int photos, String? when, int memories) =>
      '${footageCount(videos, photos)} sorted, $memories memories found${when == null ? '' : ' · last sorted $when'}';
  @override
  String get hostingNever => 'Nothing sorted yet. Tap "Hand it all over" to start.';
  @override
  String hostingStep(String step, String? file, int index, int count) => switch (step) {
        'hosting.find' => 'Finding the videos and photos',
        'hosting.look' => 'Looking at ${index + 1} of $count${file == null ? '' : ' · $file'}',
        'hosting.memories' => 'Finding the memories',
        'hosting.project' =>
          'Cutting memory ${index + 1} of $count${file == null ? '' : ': $file'}',
        _ => 'Sorting',
      };
  @override
  String get hostingFolders => 'Folders kept';
  @override
  String get hostingFoldersDefault =>
      'The usual video folders (Movies, Pictures, Desktop, Documents, Downloads)';
  @override
  String get changeFolders => 'Change folders';
  @override
  String get addFolder => 'Add a folder';
  @override
  String get useDefaultFolders => 'Use the usual ones';
  @override
  String get disksPluggedIn => 'Disks plugged in (tap one to add all of it)';
  @override
  String get foldersUnreachable => 'Some folders cannot be read now';
  @override
  String get tabCategories => 'Categories';
  @override
  String get tabMemoryMap => 'Memory map';
  @override
  String get sectionFaces => 'Faces';
  @override
  String get facesSubtitle =>
      'Grouped by how alike they look, not recognised. Open one for every video they are the '
      'subject of, and give them a name.';
  @override
  String get facesEmpty => 'No faces found yet.';
  @override
  String get sectionSmart => 'Smart categories';
  @override
  String get smartSubtitle => 'Sorted by what the picture shows. Open one to preview and start a project.';
  @override
  String get smartEmpty => 'No video sorted yet.';
  @override
  String videoCount(int n) => '$n ${n == 1 ? 'video' : 'videos'}';
  @override
  String footageCount(int videos, int photos) => photos == 0
      ? videoCount(videos)
      : videos == 0
          ? '$photos ${photos == 1 ? 'photo' : 'photos'}'
          : '${videoCount(videos)} · $photos ${photos == 1 ? 'photo' : 'photos'}';
  @override
  String get unverifiedBadge => 'Rules not yet checked';
  @override
  String get removedCategories => 'Removed categories';
  @override
  String get restore => 'Restore';
  @override
  String get removeCategory => 'Remove category';
  @override
  String removeCategoryTitle(String name) => 'Remove "$name"?';
  @override
  String get removeCategoryBody =>
      'No video will be sorted into it from now on, and the ones in it now leave it. The videos '
      'themselves are untouched, and the category can be restored at the bottom of the butler.';
  @override
  String get selectAll => 'Select all';
  @override
  String get selectNone => 'Select none';
  @override
  String selectedCount(int n) => '$n selected';
  @override
  String newProjectFrom(int videos, [int photos = 0]) => photos == 0
      ? 'New project from ${videos == 1 ? 'this video' : 'these $videos videos'}'
      : 'New project from ${footageCount(videos, photos)}';
  @override
  String get gridEmpty => 'No videos here yet.';
  @override
  String get fileTimeMark => 'file date';
  @override
  String get hostedVideos => 'Videos the butler sorted';
  @override
  String get seeInButler => 'See in the butler';
  @override
  String get mapAllMemories => 'All memories';
  @override
  String memoryKindLabel(MemoryKindId kind) => switch (kind) {
        MemoryKindId.travel => 'Trips',
        MemoryKindId.growth => 'Growing up',
        MemoryKindId.annual => 'Years',
        MemoryKindId.group => 'Groups',
      };
  @override
  String get mapEmpty =>
      'No memories yet. Once the butler has your footage, trips, people growing up, years '
      'and groups are pinned here.';
  @override
  String unpinnedMemories(int n) => '$n ${n == 1 ? 'memory has' : 'memories have'} no location';
  @override
  String memoryStatus(String? status, bool run) => switch (status) {
        'pending' => 'Waiting to be cut',
        'failed' => 'The cut failed',
        'dismissed' => 'Its project was deleted',
        _ when run => 'Cut',
        _ => 'Not cut yet',
      };
  @override
  String get generateProject => 'Cut it';
  @override
  String get regenerate => 'Cut again';
  @override
  String get orderLabel => 'Order';
  @override
  String get orderTime => 'By time';
  @override
  String get orderCategory => 'By kind: people, scenery, food, the rest';
  @override
  String memoryDates(String start, String end) =>
      start == end ? start : '$start – $end';
  @override
  String get memoryProjectNote =>
      'A memory is cut from at most 40 clips spread over its time; a year keeps each kind in '
      'proportion.';
  @override
  String get mapAttribution => 'Map: Natural Earth (public domain)';
  @override
  String memoriesHere(int n) => '$n memories here';
  @override
  String get mapLayers => 'Layers';
  @override
  String mapStats(int memories, int places, int regions) =>
      '$memories ${memories == 1 ? 'memory' : 'memories'} · $places ${places == 1 ? 'place' : 'places'} · '
      '$regions ${regions == 1 ? 'region' : 'regions'}';
  @override
  String get mapResetView => 'Show everything';
  @override
  String get memoryGenerating => 'Cutting; it will appear in your projects';
  @override
  String get mergePerson => 'Merge with another person';
  @override
  String get butlerNotesTitle => 'How these are sorted';
  @override
  String get memoryBadge => 'A memory the butler made';
  @override
  String get mergeTitle => 'The same person?';
  @override
  String get mergeBody =>
      'A face in a mask and sunglasses, or a baby and the child a year on, often land in two '
      'groups. Pick the other group to make them one person; a name given is kept.';

  // --- the built-in editor (0.2.0) -------------------------------------------
  @override
  String get edCardTitle => 'The cut';
  @override
  String get edCardBody => 'Finish the cut here and export a video, or export a 剪映 or CapCut project file to carry on there.';
  @override
  String get edEditInApp => 'Edit in the app';
  @override
  String get edExportProject => 'Export';
  @override
  String edEditedOn(String date) => 'last edited $date';
  @override
  String get edExportsTitle => 'Exported';
  @override
  String edExportKind(String kind) => switch (kind) {
        'video' => 'Video',
        'jianying' => '剪映 draft',
        'capcut' => 'CapCut draft',
        'otio' => 'OTIO timeline',
        'clips' => 'Clips',
        _ => kind,
      };
  @override
  String get edOpening => 'Opening the cut…';
  @override
  String edEdited(int percent) => 'Edited · $percent% of the footage changed';
  @override
  String get edAsMade => 'The AI cut, not yet changed';
  @override
  String get edUndo => 'Undo';
  @override
  String get edRedo => 'Redo';
  @override
  String get edExport => 'Export';
  @override
  String get edTitle => 'Title';
  @override
  String get edRevertAi => 'Back to the AI cut';
  @override
  String get edRevertAiBody => 'Go back to the cut the AI made. This too can be undone.';
  @override
  String get edRestartFromRun => 'Start again from the latest cut';
  @override
  String get edRestartBody => 'Drop the changes and start again from the project\'s latest cut. The current version is kept in the project folder (cut.previous).';
  @override
  String get edNewerRun => 'This project was cut again since you started editing; you are editing the earlier version.';
  @override
  String get edPlay => 'Play (space)';
  @override
  String get edPause => 'Pause (space)';
  @override
  String get edPrevFrame => 'Previous frame';
  @override
  String get edNextFrame => 'Next frame';
  @override
  String get edZoomIn => 'Zoom in';
  @override
  String get edZoomOut => 'Zoom out';
  @override
  String get edBack => 'Back';
  @override
  String get edSplit => 'Split';
  @override
  String get edDelete => 'Delete';
  @override
  String get edSpeed => 'Speed';
  @override
  String get edVolume => 'Volume';
  @override
  String get edDuration => 'Length';
  @override
  String get edAdjust => 'Adjust';
  @override
  String get edPosition => 'Position';
  @override
  String get edBackground => 'Background';
  @override
  String get edBackgroundAll => 'Background (every clip)';
  @override
  String get edDuplicate => 'Duplicate';
  @override
  String get edMoveEarlier => 'Earlier';
  @override
  String get edMoveLater => 'Later';
  @override
  String get edTransition => 'Transition';
  @override
  String get edTransitionAll => 'All transitions';
  @override
  String get edEditText => 'Edit';
  @override
  String get edTextToPlayhead => 'Move here';
  @override
  String get edReplaceMusic => 'Replace';
  @override
  String get edAddClip => 'Add a clip';
  @override
  String get edText => 'Text';
  @override
  String get edMusic => 'Music';
  @override
  String get edCanvas => 'Canvas';
  @override
  String get edSoundOff => 'Mute clips';
  @override
  String get edSoundOn => 'Unmute clips';
  @override
  String get edNewText => 'Text';
  @override
  String get edSpeedNote => 'The clip plays this many times as fast; its sound keeps its pitch.';
  @override
  String get edMute => 'Mute';
  @override
  String get edMuteAll => 'Mute every clip';
  @override
  String get edTransCut => 'None';
  @override
  String get edTransXfade => 'Dissolve';
  @override
  String get edTransFade => 'Dip to black';
  @override
  String get edTransWipeLeft => 'Wipe left';
  @override
  String get edTransWipeRight => 'Wipe right';
  @override
  String get edLength => 'Length';
  @override
  String get edApplyToAll => 'Apply to all';
  @override
  String get edReset => 'Reset';
  @override
  String get edBrightness => 'Brightness';
  @override
  String get edContrast => 'Contrast';
  @override
  String get edSaturation => 'Saturation';
  @override
  String get edScale => 'Scale';
  @override
  String get edMoveX => 'Across';
  @override
  String get edMoveY => 'Down';
  @override
  String get edRotate => 'Rotate';
  @override
  String get edRotate90 => 'Turn 90°';
  @override
  String get edRatio => 'Shape';
  @override
  String get edRatioOriginal => 'Original';
  @override
  String get edBgBlack => 'Black';
  @override
  String get edBgBlur => 'Blur';
  @override
  String get edTextHint => 'Type the text';
  @override
  String get edDone => 'Done';
  @override
  String get edFontSize => 'Size';
  @override
  String get edBold => 'Bold';
  @override
  String get edOutline => 'Outline';
  @override
  String get edTextBox => 'Box';
  @override
  String get edPlace => 'Place';
  @override
  String get edPlaceTop => 'Top';
  @override
  String get edPlaceMiddle => 'Middle';
  @override
  String get edPlaceBottom => 'Bottom';
  @override
  String get edDragTextHint => 'You can also drag the text in the preview.';
  @override
  String get edFadeIn => 'Fade in';
  @override
  String get edFadeOut => 'Fade out';
  @override
  String get edRemoveMusic => 'Remove';
  @override
  String get edFootageNote => 'Pick a stretch of this project\'s footage; it goes in after the clip at the playhead.';
  @override
  String edUsedTimes(int n) => n == 1 ? '1 stretch is in the cut' : '$n stretches are in the cut';
  @override
  String get edNoStretches => 'Nothing usable in it';
  @override
  String get edExportVideo => 'Export a video';
  @override
  String get edLosslessTitle => 'Lossless export';
  @override
  String get edLosslessChecking => 'Checking whether the cut can leave without re-encoding…';
  @override
  String get edLosslessButton => 'Export without re-encoding';
  @override
  String edLosslessNote(double extraSeconds) => extraSeconds < 0.05
      ? 'The footage\'s own data, copied: exactly as filmed, done in seconds.'
      : 'The footage\'s own data, copied: exactly as filmed, done in seconds. Each clip starts at the keyframe before it and ends where its frames are whole, ${extraSeconds.toStringAsFixed(1)} s more in all.';
  @override
  String get edLosslessBlocked => 'This cut needs re-encoding, because:';
  @override
  String get edClipsButton => 'Export each clip as its own file';
  @override
  String get edClipsNote => 'Clips as filmed are copied, the rest rendered one by one; text, transitions and music belong to the whole cut and are left out.';
  @override
  String edClipsDone(int copied, int rendered) => '$copied copied as filmed, $rendered rendered';
  @override
  String get edNoVideoExport => 'This device cannot write a video yet (this build has no video encoder); export a project file below.';
  @override
  String get edResolution => 'Size';
  @override
  String get edFrameRate => 'Frame rate';
  @override
  String get edFrameRateSource => 'As filmed';
  @override
  String get edCodec => 'Codec';
  @override
  String get edCodecH264 => 'H.264 (plays anywhere)';
  @override
  String get edCodecHevc => 'HEVC (smaller)';
  @override
  String get edExportDrafts => 'Export a project file';
  @override
  String get edDraftNote => 'A project file keeps every cut as a decision, to carry on changing in another editor.';
  @override
  String get edDraftJianying => '剪映 draft';
  @override
  String get edDraftCapcut => 'CapCut draft';
  @override
  String get edDraftOtio => 'OTIO timeline';
  @override
  String get edDraftOtioNote => 'For DaVinci Resolve, Premiere and the like';
  @override
  String edEditorMissing(String name) => '$name\'s draft folder was not found; install it and open it once';
  @override
  String get edExporting => 'Exporting the video…';
  @override
  String edFrames(int done, int total) => 'frame $done of $total';
  @override
  String get edThermal => 'The device is warm, so the export is going a little slower.';
  @override
  String get edExported => 'Exported';
  @override
  String get edExportedTo => 'Saved to';
  @override
  String get edShowInFolder => 'Show in folder';
  @override
  String edExportFailed(String error) => 'The export failed: $error';
}
