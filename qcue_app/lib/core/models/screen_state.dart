// QCue S4-R3: every data-driven screen renders exactly one of these. The
// offline banner is orthogonal and can co-occur with [Data] (via `stale`).
sealed class ScreenState<T> {
  const ScreenState();
}

class Loading<T> extends ScreenState<T> {
  const Loading();
}

class Empty<T> extends ScreenState<T> {
  const Empty();
}

class ErrorState<T> extends ScreenState<T> {
  const ErrorState(this.message, {this.canRetry = true});
  final String message;
  final bool canRetry;
}

class Data<T> extends ScreenState<T> {
  const Data(this.value, {this.stale = false});
  final T value;
  final bool stale; // true when served from cache while offline
}
