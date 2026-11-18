// xml2js-compatible XML → object parser for scripts (D59): `require('xml2js')`.
//
// The npm package needs Node streams (sax), so this is a small parser with the
// same output shape and the options scripts use: explicitArray, explicitRoot,
// mergeAttrs, ignoreAttrs, attrkey, charkey, explicitCharkey, trim, normalize,
// normalizeTags, emptyTag. Builder is not provided.
'use strict';

var DEFAULTS = {
  explicitArray: true,
  explicitRoot: true,
  mergeAttrs: false,
  ignoreAttrs: false,
  attrkey: '$',
  charkey: '_',
  explicitCharkey: false,
  trim: false,
  normalize: false,
  normalizeTags: false,
  emptyTag: '',
};

var ENTITIES = { lt: '<', gt: '>', amp: '&', quot: '"', apos: "'" };

function decode(s) {
  return s.replace(/&(#x[0-9a-fA-F]+|#[0-9]+|[a-zA-Z]+);/g, function (all, e) {
    if (e.charAt(0) === '#') {
      var n = e.charAt(1) === 'x' || e.charAt(1) === 'X' ? parseInt(e.slice(2), 16) : parseInt(e.slice(1), 10);
      return isNaN(n) ? all : String.fromCodePoint(n);
    }
    return Object.prototype.hasOwnProperty.call(ENTITIES, e) ? ENTITIES[e] : all;
  });
}

function fail(msg, pos) {
  var e = new Error(msg + (pos !== undefined ? ' (at character ' + pos + ')' : ''));
  e.name = 'Error';
  return e;
}

// XML → tree of { name, attrs, children: [node | {text, cdata}] }.
function tokenize(xml) {
  var i = 0;
  var n = xml.length;
  var root = { name: null, attrs: {}, children: [] };
  var stack = [root];
  var nameRe = /[^\s\/>=]+/y;

  function top() {
    return stack[stack.length - 1];
  }
  function skipTo(str) {
    var j = xml.indexOf(str, i);
    if (j < 0) throw fail('Unclosed ' + JSON.stringify(xml.substr(i, 10)), i);
    i = j + str.length;
  }

  while (i < n) {
    var lt = xml.indexOf('<', i);
    if (lt < 0) lt = n;
    if (lt > i) {
      if (stack.length === 1) {
        if (xml.slice(i, lt).trim()) throw fail('Non-whitespace before first tag', i);
      } else top().children.push({ text: decode(xml.slice(i, lt)) });
      i = lt;
      continue;
    }
    if (xml.startsWith('<?', i)) {
      skipTo('?>');
    } else if (xml.startsWith('<!--', i)) {
      skipTo('-->');
    } else if (xml.startsWith('<![CDATA[', i)) {
      var end = xml.indexOf(']]>', i);
      if (end < 0) throw fail('Unclosed CDATA', i);
      top().children.push({ text: xml.slice(i + 9, end), cdata: true });
      i = end + 3;
    } else if (xml.startsWith('<!', i)) {
      // DOCTYPE (with an optional internal subset).
      var depth = 0;
      for (i += 2; i < n; i++) {
        var c = xml.charAt(i);
        if (c === '[') depth++;
        else if (c === ']') depth--;
        else if (c === '>' && depth <= 0) break;
      }
      i++;
    } else if (xml.charAt(i + 1) === '/') {
      var close = xml.indexOf('>', i);
      if (close < 0) throw fail('Unclosed tag', i);
      var cname = xml.slice(i + 2, close).trim();
      var open = stack.pop();
      if (!open || open.name !== cname) throw fail('Unexpected close tag </' + cname + '>', i);
      i = close + 1;
    } else {
      i++;
      nameRe.lastIndex = i;
      var m = nameRe.exec(xml);
      if (!m) throw fail('Invalid tag name', i);
      var node = { name: m[0], attrs: {}, children: [] };
      i = nameRe.lastIndex;
      var selfClosing = false;
      for (;;) {
        while (i < n && /\s/.test(xml.charAt(i))) i++;
        if (i >= n) throw fail('Unclosed tag <' + node.name + '>', i);
        if (xml.startsWith('/>', i)) {
          selfClosing = true;
          i += 2;
          break;
        }
        if (xml.charAt(i) === '>') {
          i++;
          break;
        }
        nameRe.lastIndex = i;
        var a = nameRe.exec(xml);
        if (!a) throw fail('Invalid attribute', i);
        i = nameRe.lastIndex;
        while (/\s/.test(xml.charAt(i))) i++;
        var value = '';
        if (xml.charAt(i) === '=') {
          i++;
          while (/\s/.test(xml.charAt(i))) i++;
          var q = xml.charAt(i);
          if (q === '"' || q === "'") {
            var qe = xml.indexOf(q, i + 1);
            if (qe < 0) throw fail('Unclosed attribute value', i);
            value = xml.slice(i + 1, qe);
            i = qe + 1;
          } else {
            var um = /[^\s>]+/y;
            um.lastIndex = i;
            var uv = um.exec(xml);
            value = uv ? uv[0] : '';
            i += value.length;
          }
        }
        node.attrs[a[0]] = decode(value);
      }
      if (stack.length === 1 && root.children.length) throw fail('Text data outside of root node', i);
      top().children.push(node);
      if (!selfClosing) stack.push(node);
    }
  }
  if (stack.length > 1) throw fail('Unclosed root tag <' + top().name + '>');
  if (!root.children.length) throw fail('Non-whitespace before first tag');
  return root.children[0];
}

function convert(node, o) {
  var obj = {};
  var hasContent = false;
  if (!o.ignoreAttrs) {
    Object.keys(node.attrs).forEach(function (k) {
      hasContent = true;
      if (o.mergeAttrs) add(obj, k, node.attrs[k], o);
      else (obj[o.attrkey] = obj[o.attrkey] || {})[k] = node.attrs[k];
    });
  }
  var text = '';
  var cdata = false;
  node.children.forEach(function (c) {
    if (c.name === undefined) {
      text += c.text;
      if (c.cdata) cdata = true;
      return;
    }
    hasContent = true;
    var name = o.normalizeTags ? c.name.toLowerCase() : c.name;
    add(obj, name, convert(c, o), o);
  });
  if (o.trim) text = text.trim();
  if (o.normalize) text = text.replace(/\s{2,}/g, ' ').trim();
  var keepText = text.length > 0 && (cdata || !/^\s*$/.test(text));
  if (!hasContent) {
    if (!keepText) return o.emptyTag;
    if (!o.explicitCharkey) return text;
  }
  if (keepText) obj[o.charkey] = text;
  return obj;
}

function add(obj, key, value, o) {
  if (!Object.prototype.hasOwnProperty.call(obj, key)) {
    obj[key] = o.explicitArray ? [value] : value;
  } else {
    if (!Array.isArray(obj[key])) obj[key] = [obj[key]];
    obj[key].push(value);
  }
}

function options(opts) {
  var o = {};
  Object.keys(DEFAULTS).forEach(function (k) {
    o[k] = opts && opts[k] !== undefined ? opts[k] : DEFAULTS[k];
  });
  return o;
}

function parse(xml, opts) {
  var o = options(opts);
  var root = tokenize(String(xml));
  var name = o.normalizeTags ? root.name.toLowerCase() : root.name;
  var value = convert(root, o);
  if (!o.explicitRoot) return value;
  var out = {};
  out[name] = value;
  return out;
}

function Parser(opts) {
  this.options = opts || {};
}
Parser.prototype.parseString = function (xml, cb) {
  var result;
  try {
    result = parse(xml, this.options);
  } catch (e) {
    cb(e);
    return;
  }
  cb(null, result);
};
Parser.prototype.parseStringPromise = function (xml) {
  var self = this;
  return new Promise(function (resolve, reject) {
    self.parseString(xml, function (err, res) {
      if (err) reject(err);
      else resolve(res);
    });
  });
};

module.exports = {
  defaults: { '0.2': DEFAULTS, '0.1': DEFAULTS },
  Parser: Parser,
  parseString: function (xml, opts, cb) {
    if (typeof opts === 'function') {
      cb = opts;
      opts = {};
    }
    new Parser(opts).parseString(xml, cb);
  },
  parseStringPromise: function (xml, opts) {
    return new Parser(opts).parseStringPromise(xml);
  },
  Builder: function () {
    throw new Error('xml2js.Builder is not available in ilixPatch scripts');
  },
};
