import 'package:flutter_test/flutter_test.dart';
import 'package:qcue_app/core/auth/qcue_oidc.dart';

void main() {
  // App Links hardening: the OAuth/OIDC redirect must be a VERIFIED https App Link bound to this signed
  // app, not the custom `qcue://` scheme (which ANY installed app can also register → callback hijack /
  // phishing). PKCE already blocks token theft, but the verified https redirect closes the residual
  // callback-stealing window. See qcue_app/android/APP_LINKS.md.
  group('QcueOidc OAuth redirect', () {
    test('is a verified https App Link under app.qcue.cn/applink, not a hijackable custom scheme', () {
      final uri = Uri.parse(QcueOidc.redirectUrl);
      expect(uri.scheme, 'https', reason: 'qcue:// is hijackable; the redirect must be an https App Link');
      expect(uri.host, 'app.qcue.cn');
      expect(uri.path, startsWith('/applink/'), reason: 'must fall under the autoVerify App Links pathPrefix');
      expect(uri.path, '/applink/oauth2redirect');
    });

    test('redirect is exactly the path the assetlinks-verified intent-filter expects', () {
      expect(QcueOidc.redirectUrl, 'https://app.qcue.cn/applink/oauth2redirect');
    });
  });
}
