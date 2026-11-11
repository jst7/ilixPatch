// ilixPatch script sandbox prelude: builds a Postman-compatible `pm` object.
//
// Loaded once per runtime, right after the vendored chai bundle (which sets
// `globalThis.chai`). Rust drives each script through:
//   __pmRun(kind, source, inputJson)  -> undefined (state kept internally)
//   __pmTick()                        -> bool (ran pending timers?)
//   __pmFinish()                      -> JSON string with the results
(function (global) {
  'use strict';

  var chai = global.chai;
  delete global.chai;

  // ---------------------------------------------------------------- helpers

  var S = null; // state of the current run

  function hasOwn(o, k) {
    return Object.prototype.hasOwnProperty.call(o, k);
  }

  function fmtErr(e) {
    if (e && typeof e === 'object') {
      var name = e.name ? String(e.name) : 'Error';
      var msg = e.message !== undefined ? String(e.message) : String(e);
      return msg ? name + ': ' + msg : name;
    }
    return String(e);
  }

  function fmtLogArg(a) {
    if (typeof a === 'string') return a;
    if (a === undefined) return 'undefined';
    if (typeof a === 'function') return '[Function' + (a.name ? ': ' + a.name : '') + ']';
    if (a instanceof Error) return fmtErr(a);
    if (typeof a === 'object' && a !== null) {
      try {
        var s = JSON.stringify(a);
        return s === undefined ? String(a) : s;
      } catch (e) {
        return String(a);
      }
    }
    return String(a);
  }

  function toVarString(v) {
    if (v === undefined || v === null) return '';
    if (typeof v === 'object') {
      try {
        return JSON.stringify(v);
      } catch (e) {
        return String(v);
      }
    }
    return String(v);
  }

  // `a.b[0].c` lookup (lodash `_.get` subset).
  function getPath(obj, path) {
    var parts = String(path)
      .replace(/\[(\w+)\]/g, '.$1')
      .split('.')
      .filter(function (p) {
        return p !== '';
      });
    var cur = obj;
    for (var i = 0; i < parts.length; i++) {
      if (cur === null || cur === undefined || typeof cur !== 'object' || !hasOwn(cur, parts[i])) {
        return { found: false };
      }
      cur = cur[parts[i]];
    }
    return { found: true, value: cur };
  }

  function guid() {
    return 'xxxxxxxx-xxxx-4xxx-yxxx-xxxxxxxxxxxx'.replace(/[xy]/g, function (c) {
      var r = (Math.random() * 16) | 0;
      return (c === 'x' ? r : (r & 0x3) | 0x8).toString(16);
    });
  }

  function dynamicVar(name) {
    switch (name) {
      case '$guid':
      case '$randomUUID':
        return guid();
      case '$timestamp':
        return String(Math.floor(Date.now() / 1000));
      case '$isoTimestamp':
        return new Date().toISOString();
      case '$randomInt':
        return String(Math.floor(Math.random() * 1001));
      default:
        return undefined;
    }
  }

  // ---------------------------------------------------------- base64 (atob/btoa)

  var B64 = 'ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/';

  function btoa(input) {
    var str = String(input);
    var out = '';
    for (var i = 0; i < str.length; i += 3) {
      var a = str.charCodeAt(i),
        b = str.charCodeAt(i + 1),
        c = str.charCodeAt(i + 2);
      if (a > 255 || b > 255 || c > 255) {
        throw new Error("btoa: string contains characters outside of the Latin1 range");
      }
      var n = (a << 16) | ((b || 0) << 8) | (c || 0);
      out += B64.charAt((n >> 18) & 63) + B64.charAt((n >> 12) & 63);
      out += i + 1 < str.length ? B64.charAt((n >> 6) & 63) : '=';
      out += i + 2 < str.length ? B64.charAt(n & 63) : '=';
    }
    return out;
  }

  function atob(input) {
    var str = String(input).replace(/[\s=]+/g, '');
    if (str.length % 4 === 1 || /[^A-Za-z0-9+/]/.test(str)) {
      throw new Error('atob: invalid base64 input');
    }
    var out = '';
    var bits = 0,
      buf = 0;
    for (var i = 0; i < str.length; i++) {
      buf = (buf << 6) | B64.indexOf(str.charAt(i));
      bits += 6;
      if (bits >= 8) {
        bits -= 8;
        out += String.fromCharCode((buf >> bits) & 0xff);
      }
    }
    return out;
  }

  // ------------------------------------------------------------ PropertyLists

  // Header-like list of {key, value}. `onChange` is called after mutations.
  function KeyValueList(pairs, onChange) {
    var list = [];
    for (var i = 0; i < pairs.length; i++) list.push({ key: String(pairs[i][0]), value: String(pairs[i][1]) });
    this._list = list;
    this._onChange = onChange || function () {};
  }
  function lc(s) {
    return String(s).toLowerCase();
  }
  function toKv(item, value) {
    if (typeof item === 'string') {
      if (value !== undefined) return { key: item, value: toVarString(value) };
      var idx = item.indexOf(':');
      if (idx < 0) return { key: item.trim(), value: '' };
      return { key: item.slice(0, idx).trim(), value: item.slice(idx + 1).trim() };
    }
    return { key: String(item.key), value: toVarString(item.value) };
  }
  KeyValueList.prototype = {
    _find: function (name) {
      var n = lc(name);
      for (var i = 0; i < this._list.length; i++) if (lc(this._list[i].key) === n) return i;
      return -1;
    },
    get: function (name) {
      var i = this._find(name);
      return i < 0 ? undefined : this._list[i].value;
    },
    one: function (name) {
      var i = this._find(name);
      return i < 0 ? undefined : { key: this._list[i].key, value: this._list[i].value };
    },
    has: function (name, value) {
      var n = lc(name);
      for (var i = 0; i < this._list.length; i++) {
        if (lc(this._list[i].key) === n && (value === undefined || this._list[i].value === String(value))) return true;
      }
      return false;
    },
    all: function () {
      return this._list.map(function (h) {
        return { key: h.key, value: h.value };
      });
    },
    count: function () {
      return this._list.length;
    },
    each: function (fn) {
      this.all().forEach(fn);
    },
    map: function (fn) {
      return this.all().map(fn);
    },
    filter: function (fn) {
      return this.all().filter(fn);
    },
    toObject: function () {
      var o = {};
      this._list.forEach(function (h) {
        o[h.key] = h.value;
      });
      return o;
    },
    toJSON: function () {
      return this.all();
    },
    toString: function () {
      return this._list
        .map(function (h) {
          return h.key + ': ' + h.value;
        })
        .join('\n');
    },
    add: function (item, value) {
      this._list.push(toKv(item, value));
      this._onChange();
    },
    upsert: function (item, value) {
      var kv = toKv(item, value);
      var i = this._find(kv.key);
      if (i < 0) this._list.push(kv);
      else this._list[i].value = kv.value;
      this._onChange();
    },
    remove: function (name) {
      var n = typeof name === 'object' && name !== null ? lc(name.key) : lc(name);
      var self = this;
      this._list = this._list.filter(function (h) {
        return typeof name === 'function' ? !name(h) : lc(h.key) !== n;
      });
      self._onChange();
    },
    clear: function () {
      this._list = [];
      this._onChange();
    },
    _pairs: function () {
      return this._list.map(function (h) {
        return [h.key, h.value];
      });
    },
  };

  // ---------------------------------------------------------------------- Url

  function Url(raw, onChange) {
    this._raw = String(raw);
    this._onChange = onChange || function () {};
  }
  function splitUrl(raw) {
    var hash = '';
    var h = raw.indexOf('#');
    if (h >= 0) {
      hash = raw.slice(h);
      raw = raw.slice(0, h);
    }
    var q = raw.indexOf('?');
    if (q < 0) return { base: raw, query: '', hash: hash };
    return { base: raw.slice(0, q), query: raw.slice(q + 1), hash: hash };
  }
  Url.prototype = {
    toString: function () {
      return this._raw;
    },
    toJSON: function () {
      return this._raw;
    },
    update: function (raw) {
      this._raw = String(raw);
      this._onChange();
    },
    getRaw: function () {
      return this._raw;
    },
    getQueryString: function () {
      return splitUrl(this._raw).query;
    },
    getHost: function () {
      var m = /^(?:[a-z][a-z0-9+.-]*:\/\/)?([^/?#:]*)/i.exec(this._raw);
      return m ? m[1] : '';
    },
    getPath: function () {
      var base = splitUrl(this._raw).base.replace(/^[a-z][a-z0-9+.-]*:\/\//i, '');
      var i = base.indexOf('/');
      return i < 0 ? '/' : base.slice(i);
    },
    getPathWithQuery: function () {
      var q = this.getQueryString();
      return this.getPath() + (q ? '?' + q : '');
    },
    get protocol() {
      var m = /^([a-z][a-z0-9+.-]*):\/\//i.exec(this._raw);
      return m ? m[1] : undefined;
    },
    get host() {
      return this.getHost().split('.');
    },
    get path() {
      return this.getPath()
        .split('/')
        .filter(function (p) {
          return p !== '';
        });
    },
    get query() {
      var self = this;
      var parts = splitUrl(this._raw);
      var pairs = parts.query
        ? parts.query.split('&').map(function (p) {
            var i = p.indexOf('=');
            return i < 0 ? [p, ''] : [p.slice(0, i), p.slice(i + 1)];
          })
        : [];
      var list = new KeyValueList(pairs, function () {
        var p = splitUrl(self._raw);
        var qs = list._list
          .map(function (kv) {
            return kv.key + '=' + kv.value;
          })
          .join('&');
        self._raw = p.base + (qs ? '?' + qs : '') + p.hash;
        self._onChange();
      });
      return list;
    },
  };

  // ------------------------------------------------------------------ Request

  function makeRequest(input, markChanged) {
    var req = {};
    var url = new Url(input.url, markChanged);
    var method = String(input.method || 'GET');
    var headers = new KeyValueList(input.headers || [], markChanged);
    var body = makeBody(input.body, markChanged);
    Object.defineProperty(req, '__pmRequest', { value: true });
    Object.defineProperty(req, 'url', {
      enumerable: true,
      get: function () {
        return url;
      },
      set: function (v) {
        url = new Url(typeof v === 'object' && v !== null && v.raw !== undefined ? v.raw : String(v), markChanged);
        markChanged();
      },
    });
    Object.defineProperty(req, 'method', {
      enumerable: true,
      get: function () {
        return method;
      },
      set: function (v) {
        method = String(v).toUpperCase();
        markChanged();
      },
    });
    Object.defineProperty(req, 'headers', {
      enumerable: true,
      get: function () {
        return headers;
      },
    });
    Object.defineProperty(req, 'body', {
      enumerable: true,
      get: function () {
        return body;
      },
      set: function (v) {
        body = makeBody(null, markChanged);
        body.update(v);
      },
    });
    req.name = input.name;
    req.addHeader = function (h) {
      headers.add(h);
    };
    req.removeHeader = function (n) {
      headers.remove(n);
    };
    req.upsertHeader = function (h) {
      headers.upsert(h);
    };
    req.toJSON = function () {
      return { url: url.toString(), method: method, header: headers.all(), body: body.toJSON() };
    };
    req._export = function () {
      return { method: method, url: url.toString(), headers: headers._pairs(), body: body._export() };
    };
    return req;
  }

  function makeBody(input, markChanged) {
    var mode = input ? input.mode : undefined;
    var raw = input && input.raw !== undefined ? input.raw : undefined;
    var form = new KeyValueList((input && input.urlencoded) || [], function () {
      mode = 'urlencoded';
      markChanged();
    });
    var language = input && input.language;
    var body = {
      update: function (v) {
        if (typeof v === 'string') {
          mode = 'raw';
          raw = v;
        } else if (v && typeof v === 'object') {
          if (v.mode === 'urlencoded' || Array.isArray(v.urlencoded)) {
            mode = 'urlencoded';
            form = new KeyValueList(
              (v.urlencoded || []).map(function (kv) {
                return [kv.key, kv.value];
              }),
              function () {
                markChanged();
              }
            );
          } else {
            mode = 'raw';
            raw = v.raw !== undefined ? String(v.raw) : JSON.stringify(v);
          }
        }
        markChanged();
      },
      isEmpty: function () {
        return mode === undefined || (mode === 'raw' && !raw) || (mode === 'urlencoded' && form.count() === 0);
      },
      toString: function () {
        if (mode === 'urlencoded')
          return form._list
            .map(function (kv) {
              return kv.key + '=' + kv.value;
            })
            .join('&');
        return raw === undefined ? '' : raw;
      },
      toJSON: function () {
        if (mode === 'urlencoded') return { mode: mode, urlencoded: form.all() };
        if (mode === 'raw') return { mode: mode, raw: raw };
        return {};
      },
      _export: function () {
        if (mode === 'urlencoded') return { mode: mode, urlencoded: form._pairs() };
        if (mode === 'raw') return { mode: mode, raw: raw, language: language };
        return null;
      },
    };
    Object.defineProperty(body, 'mode', {
      enumerable: true,
      get: function () {
        return mode;
      },
      set: function (v) {
        mode = v;
        markChanged();
      },
    });
    Object.defineProperty(body, 'raw', {
      enumerable: true,
      get: function () {
        return raw;
      },
      set: function (v) {
        mode = 'raw';
        raw = String(v);
        markChanged();
      },
    });
    Object.defineProperty(body, 'urlencoded', {
      enumerable: true,
      get: function () {
        return form;
      },
    });
    return body;
  }

  // ----------------------------------------------------------------- Response

  function makeResponse(input) {
    var headers = new KeyValueList(input.headers || []);
    var body = String(input.body || '');
    var resp = {
      code: input.code,
      status: input.status,
      responseTime: input.time,
      responseSize: input.size,
      headers: headers,
      text: function () {
        return body;
      },
      json: function () {
        try {
          return JSON.parse(body);
        } catch (e) {
          var err = new Error(e && e.message ? e.message : String(e));
          err.name = 'JSONError';
          throw err;
        }
      },
      reason: function () {
        return input.status;
      },
      size: function () {
        return { body: body.length, header: 0, total: input.size };
      },
      toJSON: function () {
        return { code: input.code, status: input.status, header: headers.all(), body: body, responseTime: input.time };
      },
    };
    Object.defineProperty(resp, 'stream', {
      get: function () {
        return body;
      },
    });
    Object.defineProperty(resp, '__pmResponse', { value: true });
    Object.defineProperty(resp, 'to', {
      get: function () {
        return chai.expect(resp).to;
      },
    });
    Object.defineProperty(resp, 'not', {
      get: function () {
        return chai.expect(resp).not;
      },
    });
    return resp;
  }

  // ------------------------------------------------------------- chai plugin

  function isResp(o) {
    return !!(o && o.__pmResponse === true);
  }
  function isReq(o) {
    return !!(o && o.__pmRequest === true);
  }

  chai.use(function (chai, utils) {
    var A = chai.Assertion;

    function resp(a, what) {
      var o = a._obj;
      if (!isResp(o)) {
        throw new chai.AssertionError('expected a response object to assert ' + what + ' on');
      }
      return o;
    }

    A.addMethod('status', function (expected) {
      var r = resp(this, 'status');
      if (typeof expected === 'number') {
        this.assert(
          r.code === expected,
          'expected response to have status code #{exp} but got #{act}',
          'expected response to not have status code #{act}',
          expected,
          r.code
        );
      } else {
        this.assert(
          r.status === expected,
          'expected response to have status reason #{exp} but got #{act}',
          'expected response to not have status reason #{act}',
          expected,
          r.status
        );
      }
    });

    function statusProp(name, test, desc) {
      var fn = function () {
        var r = resp(this, name);
        this.assert(
          test(r.code),
          'expected response code to be ' + desc + ' but found #{act}',
          'expected response code to not be ' + desc + ' but found #{act}',
          undefined,
          r.code
        );
      };
      if (name === 'ok') {
        A.overwriteProperty('ok', function (_super) {
          return function () {
            if (isResp(this._obj)) fn.call(this);
            else _super.call(this);
          };
        });
      } else {
        A.addProperty(name, fn);
      }
    }
    statusProp('info', function (c) { return c >= 100 && c < 200; }, '1XX');
    statusProp('success', function (c) { return c >= 200 && c < 300; }, '2XX');
    statusProp('redirection', function (c) { return c >= 300 && c < 400; }, '3XX');
    statusProp('clientError', function (c) { return c >= 400 && c < 500; }, '4XX');
    statusProp('serverError', function (c) { return c >= 500 && c < 600; }, '5XX');
    statusProp('error', function (c) { return c >= 400 && c < 600; }, '4XX or 5XX');
    statusProp('ok', function (c) { return c === 200; }, '200');
    statusProp('accepted', function (c) { return c === 202; }, '202');
    statusProp('badRequest', function (c) { return c === 400; }, '400');
    statusProp('unauthorized', function (c) { return c === 401; }, '401');
    statusProp('forbidden', function (c) { return c === 403; }, '403');
    statusProp('notFound', function (c) { return c === 404; }, '404');
    statusProp('rateLimited', function (c) { return c === 429; }, '429');

    A.addProperty('withBody', function () {
      var r = resp(this, 'withBody');
      this.assert(r.text().length > 0, 'expected response to have content in body', 'expected response to not have content in body');
    });

    A.addProperty('json', function () {
      var r = resp(this, 'json');
      var ok = true;
      try {
        JSON.parse(r.text());
      } catch (e) {
        ok = false;
      }
      this.assert(ok, 'expected response body to be a valid json', 'expected response body not to be a valid json');
    });

    A.addMethod('header', function (name, value) {
      var o = this._obj;
      if (!isResp(o) && !isReq(o)) throw new chai.AssertionError('expected a response or request object to assert header on');
      var actual = o.headers.get(name);
      var has = o.headers.has(name);
      if (arguments.length < 2) {
        this.assert(has, "expected response to have header with key '" + name + "'", "expected response to not have header with key '" + name + "'");
      } else {
        this.assert(
          has && actual === String(value),
          "expected '" + name + "' response header to be #{exp} but got #{act}",
          "expected '" + name + "' response header to not be #{exp}",
          String(value),
          actual
        );
      }
    });

    A.addMethod('body', function (expected) {
      var r = resp(this, 'body');
      var text = r.text();
      if (arguments.length === 0) {
        this.assert(text.length > 0, 'expected response to have content in body', 'expected response to not have content in body');
      } else if (expected instanceof RegExp) {
        this.assert(expected.test(text), 'expected response body text to match #{exp}', 'expected response body text to not match #{exp}', expected, text);
      } else if (typeof expected === 'string') {
        this.assert(text === expected, 'expected response body to equal #{exp} but got #{act}', 'expected response body to not equal #{exp}', expected, text, true);
      } else {
        var parsed;
        try {
          parsed = JSON.parse(text);
        } catch (e) {
          parsed = undefined;
        }
        this.assert(utils.eql(parsed, expected), 'expected response body json to equal #{exp} but got #{act}', 'expected response body json to not equal #{exp}', expected, parsed, true);
      }
    });

    A.addMethod('jsonBody', function (path, value) {
      var r = resp(this, 'jsonBody');
      var parsed,
        valid = true;
      try {
        parsed = JSON.parse(r.text());
      } catch (e) {
        valid = false;
      }
      if (arguments.length === 0) {
        this.assert(valid, 'expected response body to be a valid json', 'expected response body not to be a valid json');
        return;
      }
      if (!valid) {
        this.assert(false, 'expected response body to be a valid json', 'expected response body to be a valid json');
        return;
      }
      if (typeof path === 'object' && path !== null) {
        this.assert(utils.eql(parsed, path), 'expected response body json to equal #{exp} but got #{act}', 'expected response body json to not equal #{exp}', path, parsed, true);
        return;
      }
      var got = getPath(parsed, path);
      if (arguments.length === 1) {
        this.assert(got.found, "expected #{act} in response to contain property '" + path + "'", "expected #{act} in response to not contain property '" + path + "'", undefined, parsed);
      } else {
        this.assert(
          got.found && utils.eql(got.value, value),
          "expected response body json at '" + path + "' to contain #{exp} but got #{act}",
          "expected response body json at '" + path + "' to not contain #{exp}",
          value,
          got.value,
          true
        );
      }
    });

    A.addMethod('jsonSchema', function () {
      throw new Error('jsonSchema assertions are not supported yet');
    });
  });

  // ---------------------------------------------------------------- variables

  function mapFrom(pairs) {
    var m = new Map();
    (pairs || []).forEach(function (p) {
      m.set(String(p[0]), String(p[1]));
    });
    return m;
  }

  function mapToObject(m) {
    var o = {};
    m.forEach(function (v, k) {
      o[k] = v;
    });
    return o;
  }

  function makeScope(map, scope) {
    return {
      get: function (k) {
        k = String(k);
        return map.has(k) ? map.get(k) : undefined;
      },
      set: function (k, v) {
        k = String(k);
        var s = toVarString(v);
        map.set(k, s);
        S.updates.push({ scope: scope, key: k, value: s });
      },
      unset: function (k) {
        k = String(k);
        map.delete(k);
        S.updates.push({ scope: scope, key: k, value: null });
      },
      has: function (k) {
        return map.has(String(k));
      },
      clear: function () {
        var keys = Array.from(map.keys());
        for (var i = 0; i < keys.length; i++) this.unset(keys[i]);
      },
      toObject: function () {
        return mapToObject(map);
      },
      toJSON: function () {
        return mapToObject(map);
      },
      replaceIn: function (str) {
        return replaceWith(str, function (k) {
          return map.has(k) ? map.get(k) : undefined;
        });
      },
    };
  }

  function replaceWith(str, lookup) {
    if (str === undefined || str === null) return str;
    return String(str).replace(/\{\{([^{}]+)\}\}/g, function (all, name) {
      var key = name.trim();
      var v = lookup(key);
      if (v === undefined && key.charAt(0) === '$') v = dynamicVar(key);
      return v === undefined ? all : typeof v === 'string' ? v : toVarString(v);
    });
  }

  // ------------------------------------------------------------------- pm

  function buildPm(kind, input) {
    var globals = mapFrom(input.globals);
    var collection = mapFrom(input.collection);
    var environment = mapFrom(input.environment);
    var local = mapFrom(input.local);
    var data = input.data || {};

    function resolve(k) {
      k = String(k);
      if (local.has(k)) return local.get(k);
      if (hasOwn(data, k)) return data[k];
      if (environment.has(k)) return environment.get(k);
      if (collection.has(k)) return collection.get(k);
      if (globals.has(k)) return globals.get(k);
      return undefined;
    }

    var envScope = makeScope(environment, 'environment');
    if (input.envName !== null && input.envName !== undefined) envScope.name = input.envName;
    var globalScope = makeScope(globals, 'global');
    var collectionScope = makeScope(collection, 'collection');

    var variables = {
      get: resolve,
      has: function (k) {
        return resolve(k) !== undefined;
      },
      set: function (k, v) {
        k = String(k);
        var s = toVarString(v);
        local.set(k, s);
        S.updates.push({ scope: 'local', key: k, value: s });
      },
      unset: function (k) {
        k = String(k);
        local.delete(k);
        S.updates.push({ scope: 'local', key: k, value: null });
      },
      toObject: function () {
        var o = {};
        [globals, collection, environment].forEach(function (m) {
          m.forEach(function (v, k) {
            o[k] = v;
          });
        });
        Object.keys(data).forEach(function (k) {
          o[k] = data[k];
        });
        local.forEach(function (v, k) {
          o[k] = v;
        });
        return o;
      },
      replaceIn: function (str) {
        return replaceWith(str, resolve);
      },
    };

    var iterationData = {
      get: function (k) {
        return hasOwn(data, k) ? data[k] : undefined;
      },
      has: function (k) {
        return hasOwn(data, k);
      },
      toObject: function () {
        return JSON.parse(JSON.stringify(data));
      },
      toJSON: function () {
        return data;
      },
    };

    var request = null;
    if (input.request) {
      request = makeRequest(input.request, function () {
        S.requestChanged = true;
      });
    }
    var response = input.response ? makeResponse(input.response) : null;

    function setNextRequest(name) {
      S.next = { name: name === null || name === undefined ? null : String(name) };
    }

    function test(name, fn) {
      var entry = { name: String(name), passed: true, error: null, skipped: false };
      S.tests.push(entry);
      if (typeof fn !== 'function') {
        entry.passed = false;
        entry.error = 'test function is not a function';
        return pm;
      }
      function fail(e) {
        entry.passed = false;
        entry.error = fmtErr(e);
      }
      try {
        if (fn.length > 0) {
          // Callback style: pm.test("x", function (done) { ... done(); })
          entry.pending = true;
          fn(function (err) {
            if (!entry.pending) return;
            entry.pending = false;
            if (err) fail(err);
          });
        } else {
          var ret = fn();
          if (ret && typeof ret.then === 'function') {
            entry.pending = true;
            ret.then(
              function () {
                entry.pending = false;
              },
              function (e) {
                entry.pending = false;
                fail(e);
              }
            );
          }
        }
      } catch (e) {
        entry.pending = false;
        fail(e);
      }
      return pm;
    }
    test.skip = function (name) {
      S.tests.push({ name: String(name), passed: true, error: null, skipped: true });
      return pm;
    };
    test.index = function () {
      return S.tests.length;
    };

    var expect = function (v, msg) {
      return chai.expect(v, msg);
    };
    expect.fail = chai.expect.fail;

    var pm = {
      info: {
        eventName: kind === 'pre_request' ? 'prerequest' : 'test',
        iteration: input.iteration,
        iterationCount: input.iterationCount,
        requestName: input.requestName === null ? undefined : input.requestName,
        requestId: '',
      },
      test: test,
      expect: expect,
      environment: envScope,
      globals: globalScope,
      collectionVariables: collectionScope,
      variables: variables,
      iterationData: iterationData,
      execution: {
        skipRequest: function () {
          if (kind === 'pre_request') S.skip = true;
        },
        setNextRequest: setNextRequest,
        location: [],
      },
      setNextRequest: setNextRequest,
      sendRequest: function () {
        throw new Error('pm.sendRequest is not supported yet');
      },
      require: requireFn,
    };
    if (request) pm.request = request;
    if (response) pm.response = response;

    var postman = {
      setNextRequest: setNextRequest,
      setEnvironmentVariable: function (k, v) {
        envScope.set(k, v);
      },
      getEnvironmentVariable: function (k) {
        return envScope.get(k);
      },
      clearEnvironmentVariable: function (k) {
        envScope.unset(k);
      },
      clearEnvironmentVariables: function () {
        envScope.clear();
      },
      setGlobalVariable: function (k, v) {
        globalScope.set(k, v);
      },
      getGlobalVariable: function (k) {
        return globalScope.get(k);
      },
      clearGlobalVariable: function (k) {
        globalScope.unset(k);
      },
      clearGlobalVariables: function () {
        globalScope.clear();
      },
      getVariable: function (k) {
        return resolve(k);
      },
      setVariable: function (k, v) {
        variables.set(k, v);
      },
      getResponseHeader: function (name) {
        return response ? response.headers.get(name) : undefined;
      },
    };

    var legacy = {
      tests: {},
      responseBody: response ? response.text() : undefined,
      responseCode: response ? { code: response.code, name: response.status, detail: response.status } : undefined,
      responseTime: response ? response.responseTime : undefined,
      responseHeaders: response ? response.headers.toObject() : undefined,
      environment: envScope.toObject(),
      globals: globalScope.toObject(),
      data: iterationData.toObject(),
      iteration: input.iteration,
      request: request
        ? {
            url: request.url.toString(),
            method: request.method,
            headers: request.headers.toObject(),
            data: request.body.toString(),
            name: input.requestName,
          }
        : undefined,
    };

    return { pm: pm, postman: postman, legacy: legacy };
  }

  // ---------------------------------------------------------- global builtins

  function requireFn(name) {
    if (name === 'chai') return chai;
    throw new Error("module '" + name + "' is not available yet");
  }

  function log(level) {
    return function () {
      if (!S) return;
      var line = Array.prototype.map.call(arguments, fmtLogArg).join(' ');
      S.logs.push(level ? '[' + level + '] ' + line : line);
    };
  }

  var timerSeq = 0;
  function setTimeoutFn(fn, delay) {
    var args = Array.prototype.slice.call(arguments, 2);
    var id = ++timerSeq;
    if (S && typeof fn === 'function') S.timers.push({ id: id, fn: fn, args: args, delay: Number(delay) || 0 });
    return id;
  }
  function clearTimeoutFn(id) {
    if (!S) return;
    S.timers = S.timers.filter(function (t) {
      return t.id !== id;
    });
  }

  global.console = {
    log: log(''),
    info: log('info'),
    warn: log('warn'),
    error: log('error'),
    debug: log('debug'),
    trace: log('debug'),
  };
  global.atob = atob;
  global.btoa = btoa;
  global.require = requireFn;
  global.setTimeout = setTimeoutFn;
  global.clearTimeout = clearTimeoutFn;
  global.setImmediate = function (fn) {
    return setTimeoutFn(fn, 0);
  };
  global.clearImmediate = clearTimeoutFn;

  // ------------------------------------------------------------- run protocol

  var PARAMS = [
    'pm',
    'postman',
    'tests',
    'responseBody',
    'responseCode',
    'responseTime',
    'responseHeaders',
    'environment',
    'globals',
    'data',
    'iteration',
    'request',
  ];

  var baselineGlobals = null;

  global.__pmRun = function (kind, source, inputJson) {
    S = {
      tests: [],
      updates: [],
      logs: [],
      next: null,
      skip: false,
      requestChanged: false,
      error: null,
      timers: [],
      legacyTests: null,
      request: null,
    };
    var input = JSON.parse(inputJson);
    var built = buildPm(kind, input);
    S.legacyTests = built.legacy.tests;
    S.request = built.pm.request || null;
    var fn;
    try {
      fn = new Function(PARAMS.join(','), source);
    } catch (e) {
      S.error = fmtErr(e);
      return;
    }
    try {
      var l = built.legacy;
      fn(built.pm, built.postman, l.tests, l.responseBody, l.responseCode, l.responseTime, l.responseHeaders, l.environment, l.globals, l.data, l.iteration, l.request);
    } catch (e) {
      S.error = fmtErr(e);
    }
  };

  // Runs due timers (all of them, in delay order). Returns true if any ran.
  global.__pmTick = function () {
    if (!S || S.error || S.timers.length === 0) return false;
    var batch = S.timers.slice().sort(function (a, b) {
      return a.delay - b.delay || a.id - b.id;
    });
    S.timers = [];
    for (var i = 0; i < batch.length; i++) {
      try {
        batch[i].fn.apply(null, batch[i].args);
      } catch (e) {
        S.error = fmtErr(e);
        return false;
      }
    }
    return true;
  };

  global.__pmFinish = function () {
    var st = S;
    S = null;
    cleanupGlobals();
    if (!st) return JSON.stringify({ tests: [], updates: [], logs: [], skip: false, error: 'internal: no script state' });
    var tests = st.tests.map(function (t) {
      if (t.pending) return { name: t.name, passed: false, error: 'test did not complete (callback not called or promise not settled)', skipped: false };
      return { name: t.name, passed: t.passed, error: t.error, skipped: t.skipped };
    });
    var lt = st.legacyTests || {};
    Object.keys(lt).forEach(function (k) {
      tests.push({ name: k, passed: !!lt[k], error: lt[k] ? null : 'AssertionError: expected false to be truthy', skipped: false });
    });
    return JSON.stringify({
      tests: tests,
      updates: st.updates,
      logs: st.logs,
      next: st.next,
      skip: st.skip,
      error: st.error,
      request: st.request && st.requestChanged ? st.request._export() : null,
    });
  };

  // Remove globals created by user scripts so runs don't leak into each other.
  function cleanupGlobals() {
    if (!baselineGlobals) return;
    var names = Object.getOwnPropertyNames(global);
    for (var i = 0; i < names.length; i++) {
      if (!hasOwn(baselineGlobals, names[i])) {
        try {
          delete global[names[i]];
        } catch (e) {
          /* non-configurable: ignore */
        }
      }
    }
  }

  baselineGlobals = {};
  Object.getOwnPropertyNames(global).forEach(function (n) {
    baselineGlobals[n] = true;
  });
})(globalThis);
