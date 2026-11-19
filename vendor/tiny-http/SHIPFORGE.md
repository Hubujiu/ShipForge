# HTTP body admission

This directory contains the library sources of tiny_http 0.12.0, distributed under the accompanying MIT or Apache-2.0 licenses. The source archive is the crates.io release with SHA-256 digest 389915df6413a2e74fb181895f933386023c71110878cd0825588928e64cdc82. [Upstream source tree](https://github.com/tiny-http/tiny-http/tree/212b1c45852fef2093dc1374875a9393c55eb4b9)

ShipForge adds the optional Cargo feature `defer-small-bodies`. The team server enables it so the upstream constructor uses its existing lazy reader for short Content-Length bodies as well. This lets a webhook return its admission decision after headers even when a client withholds a short body and omits Expect. Accepted requests use the ordinary request reader and response writer. The upstream eager read still applies when the feature is disabled.

The runtime change is the short-body condition in `src/request.rs`. The local build also gates an import on Unix, removes an unused private re-export, and expresses the original Send and Sync check as a compile-time assertion. These three build adjustments keep the existing warning policy on Windows and Linux.
