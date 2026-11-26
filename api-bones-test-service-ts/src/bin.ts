#!/usr/bin/env node
// SPDX-License-Identifier: MIT
// Runs the test service as this process. Startup prints one JSON line
// `{"ready":true,"url":...,"control":...}` on stdout; `POST <control>/configure`
// seeds what the services answer and `GET <control>/seen` reads back what each
// request carried. The process ends when its stdin closes.
import "./server.js";
