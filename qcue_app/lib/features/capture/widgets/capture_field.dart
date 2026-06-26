// QCue S4-R30: the always-ready capture field pinned at the bottom — a
// multiline text input + a tap-to-start / tap-to-stop mic button. Commits on
// send/submit; the mic uses an injected VoiceCaptureController seam (canned
// transcript here; real STT is S5). Hairline, flat, content-first.
import 'dart:async';

import 'package:flutter/material.dart';
import '../../../core/models/transcribe_error.dart';
import '../../../core/theme/qcue_space.dart';
import '../../../core/theme/qcue_text.dart';
import '../../../core/theme/qcue_theme.dart';
import 'voice_capture_controller.dart';

class CaptureField extends StatefulWidget {
  const CaptureField({
    super.key,
    required this.onCommit,
    required this.voice,
  });

  /// Called with (body, origin) — origin is 'capture' for typed, 'voice' for
  /// the mic.
  final void Function(String body, String origin) onCommit;
  final VoiceCaptureController voice;

  @override
  State<CaptureField> createState() => _CaptureFieldState();
}

class _CaptureFieldState extends State<CaptureField> {
  final _controller = TextEditingController();
  final _focusNode = FocusNode();
  bool _listening = false;
  bool _transcribing = false;

  void _commit() {
    final text = _controller.text.trim();
    if (text.isEmpty) return;
    widget.onCommit(text, 'capture');
    _controller.clear();
  }

  Future<void> _toggleMic() async {
    // Tap-to-stop: a second tap while recording ends the take; the transcript is
    // then fetched from the cloud STT and dropped into the editable field (D4).
    if (_listening) {
      setState(() => _transcribing = true);
      await widget.voice.stop();
      return;
    }
    setState(() {
      _listening = true;
      _transcribing = false;
    });
    var transcript = '';
    String? errorMessage;
    try {
      transcript = await widget.voice.capture();
    } on TranscribeException catch (e) {
      errorMessage = e.uiMessage; // the server's REAL reason (no key / provider / network)
    } catch (_) {
      errorMessage = 'Voice capture failed — try again.';
    } finally {
      if (mounted) {
        setState(() {
          _listening = false;
          _transcribing = false;
        });
      }
    }
    if (!mounted) return; // cancelled on dispose — no field change, no message.
    final text = transcript.trim();
    if (text.isNotEmpty) {
      // D4: load the transcript into the EDITABLE field for review — do NOT
      // auto-commit. The user edits/confirms, then taps send.
      _controller.text = text;
      _controller.selection =
          TextSelection.collapsed(offset: _controller.text.length);
      _focusNode.requestFocus();
    } else {
      // Either a real failure (errorMessage = the server's reason) or an empty
      // take (no speech / denied) — one actionable message, never the silent flip.
      ScaffoldMessenger.of(context).showSnackBar(
        SnackBar(
          key: const ValueKey('mic-no-transcript'),
          content: Text(
            errorMessage ??
                "Didn't catch any speech — try again, or check the microphone "
                    'permission.',
          ),
        ),
      );
    }
  }

  @override
  void dispose() {
    // A field torn down mid-recording must not leave the mic open — abort it.
    if (_listening) unawaited(widget.voice.cancel());
    _focusNode.dispose();
    _controller.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    return Padding(
      padding: const EdgeInsets.all(QSpace.md),
      child: Row(
        crossAxisAlignment: CrossAxisAlignment.end,
        children: [
          Expanded(
            child: Semantics(
              textField: true,
              label: 'Capture field',
              child: TextField(
                key: const ValueKey('capture-field'),
                controller: _controller,
                focusNode: _focusNode,
                minLines: 1,
                maxLines: 4,
                textInputAction: TextInputAction.send,
                onSubmitted: (_) => _commit(),
                style: QCueText.body.copyWith(color: context.q.text),
                decoration: InputDecoration(
                  hintText: 'Capture a thought…',
                  hintStyle:
                      QCueText.body.copyWith(color: context.q.text3),
                  filled: true,
                  fillColor: context.q.surface,
                  border: OutlineInputBorder(
                    borderRadius: BorderRadius.circular(QRadius.control),
                    borderSide: BorderSide(color: context.q.border),
                  ),
                  enabledBorder: OutlineInputBorder(
                    borderRadius: BorderRadius.circular(QRadius.control),
                    borderSide: BorderSide(color: context.q.border),
                  ),
                ),
              ),
            ),
          ),
          const SizedBox(width: QSpace.sm),
          ConstrainedBox(
            constraints: const BoxConstraints(minWidth: 44, minHeight: 44),
            child: IconButton(
              key: const ValueKey('mic-button'),
              // Stays enabled while recording so the user can tap to stop; disabled
              // while the clip is being transcribed.
              tooltip: _transcribing
                  ? 'Transcribing…'
                  : (_listening ? 'Tap to stop' : 'Tap to speak'),
              onPressed: _transcribing ? null : _toggleMic,
              icon: Icon(
                _transcribing
                    ? Icons.hourglass_top
                    : (_listening
                        ? Icons.stop_rounded
                        : Icons.mic_none_outlined),
                color: _listening || _transcribing
                    ? context.q.accent
                    : context.q.text2,
              ),
            ),
          ),
        ],
      ),
    );
  }
}
