#!/usr/bin/env ruby
# frozen_string_literal: true
#
# QCue S5 — one-time wiring of the iOS native layer into Runner.xcodeproj.
#
# `flutter create` only scaffolds the Runner + RunnerTests targets; it never adds
# the S5 handler sources, the Share Extension, or the WidgetKit widget. This script
# performs that surgery deterministically with the `xcodeproj` gem (the same library
# CocoaPods uses) instead of fragile hand-editing of project.pbxproj.
#
# It:
#   1. adds the 8 handler sources (+ Shared/SharedContainer.swift) to Runner,
#   2. adds the 7 handler XCTest files to RunnerTests,
#   3. creates the ShareExtension + QcueWidget app-extension targets (SharedContainer
#      compiled into all three so they agree on the App Group keys),
#   4. embeds both extensions into Runner (Embed Foundation Extensions, PlugIns) and
#      makes them Runner target dependencies,
#   5. attaches the App Group entitlements + the signing team to every target.
#
# Idempotency: it ASSUMES a pristine flutter-template pbxproj and aborts if the
# ShareExtension target already exists. To re-run:  git checkout -- Runner.xcodeproj
# && ruby tools/wire_native_targets.rb
#
# Decisions (see docs/superpowers/plans/2026-06-15-ios-native-parity-completion-plan.md):
#   Team ALNQNQS4CU (HAIXU WEN, paid) · App Group group.cn.qcue.shared ·
#   bundle ids cn.qcue.app(.share|.widget) · iOS 13/13/14 floors.

require 'xcodeproj'

IOS_DIR      = File.expand_path('..', __dir__)        # .../qcue_app/ios
PROJECT_PATH = File.join(IOS_DIR, 'Runner.xcodeproj')
TEAM         = 'ALNQNQS4CU'
APP_BUNDLE   = 'cn.qcue.app'

proj = Xcodeproj::Project.open(PROJECT_PATH)
runner       = proj.targets.find { |t| t.name == 'Runner' }       or abort 'no Runner target'
runner_tests = proj.targets.find { |t| t.name == 'RunnerTests' }  or abort 'no RunnerTests target'
abort 'ShareExtension already exists — re-run on a pristine pbxproj' if proj.targets.any? { |t| t.name == 'ShareExtension' }

main_group   = proj.main_group
runner_group = main_group['Runner']      or abort 'no Runner group'
tests_group  = main_group['RunnerTests'] or abort 'no RunnerTests group'
generated_xcconfig = proj.files.find { |f| f.path == 'Flutter/Generated.xcconfig' }

def subgroup(parent, name)
  parent[name] || parent.new_group(name, name)
end

def add_ref_once(group, filename)
  group.files.find { |f| f.display_name == filename } || group.new_file(filename)
end

# ── 1. Runner handler sources (group-relative under ios/Runner) ──────────────
{
  'Native'     => 'QcueChannels.swift',
  'Stt'        => 'SttHandler.swift',
  'Secure'     => 'SecureHandler.swift',
  'Share'      => 'ShareHandler.swift',
  'Widget'     => 'WidgetHandler.swift',
  'Notif'      => 'NotifHandler.swift',
  'Background' => 'BackgroundHandler.swift',
}.each do |dir, fname|
  ref = add_ref_once(subgroup(runner_group, dir), fname)
  runner.add_file_references([ref])
end

# ── 2. Shared/SharedContainer.swift → compiled into all three targets ────────
shared_ref = add_ref_once(subgroup(main_group, 'Shared'), 'SharedContainer.swift')
runner.add_file_references([shared_ref])

# ── 3. RunnerTests handler tests ─────────────────────────────────────────────
%w[BackgroundHandlerTests NotifHandlerTests QcueChannelsTests SecureHandlerTests
   ShareHandlerTests SttHandlerTests WidgetHandlerTests].each do |base|
  runner_tests.add_file_references([add_ref_once(tests_group, "#{base}.swift")])
end

# ── helpers for the extension targets ────────────────────────────────────────
def ensure_profile(target)
  list = target.build_configuration_list
  return if list['Profile']
  release = list['Release']
  profile = target.project.new(Xcodeproj::Project::Object::XCBuildConfiguration)
  profile.name = 'Profile'
  profile.build_settings = release.build_settings.dup
  profile.base_configuration_reference = release.base_configuration_reference
  list.build_configurations << profile
end

def make_extension(proj, name:, deployment:, swift_file:, bundle_id:, plist:, entitlements:, team:, base_xcconfig:, shared_ref:)
  ext = proj.new_target(:app_extension, name, :ios, deployment, nil, :swift)
  ensure_profile(ext)

  grp = proj.main_group[name] || proj.main_group.new_group(name, name)
  ext.add_file_references([grp.files.find { |f| f.display_name == swift_file } || grp.new_file(swift_file)])
  ext.add_file_references([shared_ref]) # SharedContainer compiled into the extension too
  [File.basename(plist), File.basename(entitlements)].each do |f|
    grp.new_file(f) unless grp.files.any? { |x| x.display_name == f }
  end

  ext.build_configurations.each do |c|
    bs = c.build_settings
    bs['PRODUCT_BUNDLE_IDENTIFIER']    = bundle_id
    bs['INFOPLIST_FILE']               = plist
    bs['CODE_SIGN_ENTITLEMENTS']       = entitlements
    bs['CODE_SIGN_STYLE']              = 'Automatic'
    bs['DEVELOPMENT_TEAM']             = team
    bs['IPHONEOS_DEPLOYMENT_TARGET']   = deployment
    bs['SWIFT_VERSION']                = '5.0'
    bs['TARGETED_DEVICE_FAMILY']       = '1,2'
    bs['GENERATE_INFOPLIST_FILE']      = 'NO'
    bs['SKIP_INSTALL']                 = 'YES'   # shipped via the host Embed phase
    bs['PRODUCT_NAME']                 = '$(TARGET_NAME)'
    bs['CURRENT_PROJECT_VERSION']      = '$(FLUTTER_BUILD_NUMBER)'
    bs['MARKETING_VERSION']            = '$(FLUTTER_BUILD_NAME)'
    bs['LD_RUNPATH_SEARCH_PATHS']      = ['$(inherited)', '@executable_path/Frameworks', '@executable_path/../../Frameworks']
    c.base_configuration_reference = base_xcconfig if base_xcconfig
  end
  ext
end

share_ext = make_extension(proj,
  name: 'ShareExtension', deployment: '13.0',
  swift_file: 'ShareViewController.swift', bundle_id: "#{APP_BUNDLE}.share",
  plist: 'ShareExtension/Info.plist', entitlements: 'ShareExtension/ShareExtension.entitlements',
  team: TEAM, base_xcconfig: generated_xcconfig, shared_ref: shared_ref)

widget_ext = make_extension(proj,
  name: 'QcueWidget', deployment: '14.0',
  swift_file: 'QcueWidget.swift', bundle_id: "#{APP_BUNDLE}.widget",
  plist: 'QcueWidget/Info.plist', entitlements: 'QcueWidget/QcueWidget.entitlements',
  team: TEAM, base_xcconfig: generated_xcconfig, shared_ref: shared_ref)

# ── 4. Runner depends on + embeds both extensions ────────────────────────────
runner.add_dependency(share_ext)
runner.add_dependency(widget_ext)

embed = runner.new_copy_files_build_phase('Embed Foundation Extensions')
embed.symbol_dst_subfolder_spec = :plug_ins   # dstSubfolderSpec = 13 (PlugIns)
[share_ext, widget_ext].each do |ext|
  bf = embed.add_file_reference(ext.product_reference, true)
  bf.settings = { 'ATTRIBUTES' => ['RemoveHeadersOnCopy'] }
end
# Move the embed phase to just BEFORE Flutter's 'Thin Binary' script phase (the
# conventional position — the .appex are in PlugIns/ before final signing).
begin
  phases = runner.build_phases
  phases.delete(embed)
  thin_i = phases.index { |p| p.display_name == 'Thin Binary' } || phases.count
  phases.insert(thin_i, embed)
rescue StandardError => e
  warn "warn: could not reorder embed phase (#{e.class}: #{e.message}); left appended"
  runner.build_phases << embed unless runner.build_phases.include?(embed)
end

# ── 5. App Group entitlements + signing team on every target ─────────────────
runner.build_configurations.each do |c|
  c.build_settings['CODE_SIGN_ENTITLEMENTS'] = 'Runner/Runner.entitlements'
  c.build_settings['DEVELOPMENT_TEAM']       = TEAM
  c.build_settings['CODE_SIGN_STYLE']        = 'Automatic'
  # The handlers expose `testHandle` behind `#if DEBUG`; the flutter template omits
  # DEBUG from the Runner target's Swift compilation conditions, so `@testable import
  # Runner` can't see it. Define it for the Debug config (standard Xcode app behavior).
  if c.name == 'Debug'
    c.build_settings['SWIFT_ACTIVE_COMPILATION_CONDITIONS'] = '$(inherited) DEBUG'
  end
end
add_ref_once(runner_group, 'Runner.entitlements')

runner_tests.build_configurations.each do |c|
  c.build_settings['DEVELOPMENT_TEAM'] = TEAM
  c.build_settings['CODE_SIGN_STYLE']  = 'Automatic'
end

attrs = (proj.root_object.attributes['TargetAttributes'] ||= {})
[share_ext, widget_ext].each do |t|
  attrs[t.uuid] = { 'CreatedOnToolsVersion' => '26.5', 'ProvisioningStyle' => 'Automatic' }
end

proj.save

puts '── wired Runner.xcodeproj ──'
proj.targets.each do |t|
  puts "target: #{t.name}  (#{t.product_type})  deps=#{t.dependencies.map { |d| d.target&.name }.join(',')}"
  srcs = t.source_build_phase&.files_references&.map(&:display_name) || []
  puts "  sources(#{srcs.size}): #{srcs.sort.join(', ')}"
end
runner_phases = runner.build_phases.map { |p| p.display_name }
puts "Runner phases: #{runner_phases.join(' | ')}"
