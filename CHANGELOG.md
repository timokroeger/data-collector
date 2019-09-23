# Changelog
All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## Unreleased
- -

## v0.5.1 - 2019-09-11
- Update dependencies
- Build docker image for arm32v7 target (compatible with rpi)
- Add InfluxDB configuration documentation

## v0.5.0
- Support for InfluxDB 2.0 API

## v0.6.2 - 2019-09-08
- Use ARMv7 alpine image for docker
- Fix host name resolution

## v0.6.1 - 2019-08-21
- Update dependencies
- Fix docker image

## v0.6.0
- ModbusRTU support
- Switch to tokio_modbus library

## v0.5.0
- Support for InfluxDB 2.0 API

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
