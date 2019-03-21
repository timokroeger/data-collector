# Changelog
All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## Unreleased

## v0.4.0
- Revert back to v0.2 behaviour. Keep-alive checking was the wrong approach
- Config code cleanup

## v0.3.1
- Fix for the fact that different OSes use different io error codes

## v0.3.0
- Use TCP keep-alive feature to detect broken connectios

## v0.2.1 - 2019-03-19
- Fix panic when no log file was specified as command line parameter

## v0.2.0 - 2019-03-16
### Added
- Configurable logging with the command line options --loglevel and --logfile

## v0.1.0 - 2019-03-16
Initial Release
