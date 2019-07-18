# Pharos Changelog

## 0.2.0 - 2019-07-18

BREAKING CHANGE:  Update dependencies. The Futures library has some changes, which introduce a breaking change. Bounded channels are no longer queue_size + 1. They now are queue_size. This means that `Observable::observe( queue_size: usize )` will now yield a `futures::channel::Receiver` of the exact `queue_size`.