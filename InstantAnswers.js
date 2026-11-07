.pragma library
.import "EverythingModel.js" as Model

// Instant answers for the search line: calculator, currency and unit
// conversion, time zones, timers, clipboard history and "ask an agent".
// Pure functions only. Service.qml owns every side effect (network, files,
// processes) and passes the data in through the context object.

// --- calculator --------------------------------------------------------------
//
// A small recursive-descent evaluator for the search line. It never calls
// eval/Function: the query is untrusted text and only the grammar below runs.
//
//   expr    := term (("+" | "-") term)*          a + b% means a * (1 + b/100)
//   term    := unary (("*" | "/" | "mod") unary | implicit-multiply)*
//   unary   := ("-" | "+") unary | power
//   power   := postfix ("^" unary)?               right-associative
//   postfix := primary ("!" | "%")*
//   primary := number | constant | func "(" args ")" | "(" expr ")"
//
// "15% of 80" is rewritten to "(15/100)*(80)" before parsing.

var CALC_FUNCTIONS = {
  sqrt: Math.sqrt, cbrt: Math.cbrt, abs: Math.abs, round: Math.round,
  floor: Math.floor, ceil: Math.ceil, exp: Math.exp, ln: Math.log,
  log: Math.log10, log10: Math.log10, log2: Math.log2,
  sin: Math.sin, cos: Math.cos, tan: Math.tan,
  asin: Math.asin, acos: Math.acos, atan: Math.atan,
  sinh: Math.sinh, cosh: Math.cosh, tanh: Math.tanh,
  min: Math.min, max: Math.max, pow: Math.pow, hypot: Math.hypot
}
var CALC_CONSTANTS = { pi: Math.PI, "π": Math.PI, e: Math.E, tau: 2 * Math.PI }
var CALC_MAX_LENGTH = 200

function calcTokenize(text) {
  var tokens = []
  var i = 0
  while (i < text.length) {
    var ch = text.charAt(i)
    if (/\s/.test(ch)) { i += 1; continue }
    var rest = text.slice(i)
    var hex = /^0x[0-9a-f]+/i.exec(rest)
    var bin = /^0b[01]+/i.exec(rest)
    var num = /^(?:\d[\d_]*(?:\.\d*)?|\.\d+)(?:e[+-]?\d+)?/i.exec(rest)
    if (hex) { tokens.push({ t: "num", v: parseInt(hex[0].slice(2), 16) }); i += hex[0].length; continue }
    if (bin) { tokens.push({ t: "num", v: parseInt(bin[0].slice(2), 2) }); i += bin[0].length; continue }
    if (num) {
      tokens.push({ t: "num", v: parseFloat(num[0].replace(/_/g, "")) })
      i += num[0].length
      continue
    }
    var word = /^[a-zπ]+[0-9]*/i.exec(rest)
    if (word) {
      var name = word[0].toLowerCase()
      // "3 x 4" multiplies; x is never a constant or function name.
      if (name === "x") tokens.push({ t: "op", v: "*" })
      else tokens.push({ t: "id", v: name })
      i += word[0].length
      continue
    }
    if (rest.indexOf("**") === 0) { tokens.push({ t: "op", v: "^" }); i += 2; continue }
    var mapped = { "×": "*", "·": "*", "÷": "/", "−": "-" }[ch] || ch
    if ("+-*/^%!(),".indexOf(mapped) >= 0) { tokens.push({ t: "op", v: mapped }); i += 1; continue }
    return null
  }
  return tokens
}

function calcParse(tokens) {
  var pos = 0
  var operators = 0
  function peek() { return tokens[pos] }
  function isOp(v) { var t = tokens[pos]; return t && t.t === "op" && t.v === v }
  function fail() { throw new Error("calc") }

  function primary() {
    var t = tokens[pos]
    if (!t) fail()
    if (t.t === "num") { pos += 1; return { v: t.v, pct: false } }
    if (t.t === "op" && t.v === "(") {
      pos += 1
      var inner = expr()
      if (!isOp(")")) fail()
      pos += 1
      operators += 1
      return { v: inner, pct: false }
    }
    if (t.t === "id") {
      pos += 1
      if (Object.prototype.hasOwnProperty.call(CALC_CONSTANTS, t.v)) {
        operators += 1
        return { v: CALC_CONSTANTS[t.v], pct: false }
      }
      if (!Object.prototype.hasOwnProperty.call(CALC_FUNCTIONS, t.v) || !isOp("(")) fail()
      pos += 1
      var args = []
      if (!isOp(")")) {
        args.push(expr())
        while (isOp(",")) { pos += 1; args.push(expr()) }
      }
      if (!isOp(")")) fail()
      pos += 1
      operators += 1
      return { v: CALC_FUNCTIONS[t.v].apply(null, args), pct: false }
    }
    fail()
  }

  function postfix() {
    var node = primary()
    while (isOp("!") || isOp("%")) {
      var op = tokens[pos].v
      pos += 1
      operators += 1
      if (op === "%") node = { v: node.v / 100, pct: true }
      else {
        var n = node.v
        if (n < 0 || n > 170 || Math.floor(n) !== n) fail()
        var f = 1
        for (var k = 2; k <= n; k++) f *= k
        node = { v: f, pct: false }
      }
    }
    return node
  }

  function power() {
    var base = postfix()
    if (isOp("^")) {
      pos += 1
      operators += 1
      var exponent = unary()
      return { v: Math.pow(base.v, exponent.v), pct: false }
    }
    return base
  }

  function unary() {
    if (isOp("-")) { pos += 1; operators += 1; var neg = unary(); return { v: -neg.v, pct: neg.pct } }
    if (isOp("+")) { pos += 1; return unary() }
    return power()
  }

  function startsOperand() {
    var t = peek()
    return t && (t.t === "num" || t.t === "id" || (t.t === "op" && t.v === "("))
  }

  function term() {
    var left = unary()
    while (true) {
      if (isOp("*") || isOp("/")) {
        var op = tokens[pos].v
        pos += 1
        operators += 1
        var right = unary()
        left = { v: op === "*" ? left.v * right.v : left.v / right.v, pct: false }
      } else if (peek() && peek().t === "id" && peek().v === "mod") {
        pos += 1
        operators += 1
        var divisor = unary()
        left = { v: ((left.v % divisor.v) + divisor.v) % divisor.v, pct: false }
      } else if (startsOperand()) {
        // Implicit multiplication: 2pi, 3(4 + 1), (1 + 2)(3).
        operators += 1
        var next = unary()
        left = { v: left.v * next.v, pct: false }
      } else break
    }
    return left
  }

  function expr() {
    var left = term()
    while (isOp("+") || isOp("-")) {
      var op = tokens[pos].v
      pos += 1
      operators += 1
      var right = term()
      // Calculator percent: 80 + 15% is 80 plus 15% of 80.
      var delta = right.pct ? left.v * right.v : right.v
      left = { v: op === "+" ? left.v + delta : left.v - delta, pct: false }
    }
    return left.v
  }

  var value = expr()
  if (pos !== tokens.length) fail()
  return { value: value, operators: operators }
}

function formatCalcNumber(value) {
  if (!isFinite(value)) return ""
  if (value === 0) return "0"
  var abs = Math.abs(value)
  if (abs >= 1e15 || abs < 1e-9) {
    return value.toExponential(9).replace(/\.?0+e/, "e").replace("e+", "e")
  }
  var rounded = Number(value.toPrecision(12))
  var parts = String(rounded).split(".")
  if (parts[0].indexOf("e") >= 0) return String(rounded)
  var sign = parts[0].charAt(0) === "-" ? "-" : ""
  var whole = parts[0].replace("-", "").replace(/\B(?=(\d{3})+(?!\d))/g, ",")
  return sign + whole + (parts.length > 1 ? "." + parts[1] : "")
}

// Plain machine value for the clipboard: no grouping separators.
function plainCalcNumber(value) {
  if (!isFinite(value)) return ""
  var abs = Math.abs(value)
  if (abs !== 0 && (abs >= 1e15 || abs < 1e-9)) return value.toExponential(9).replace(/\.?0+e/, "e")
  return String(Number(value.toPrecision(12)))
}

// Returns { value, display, copy, expression } when the query is a
// calculation, else null. A bare number or a word is not a calculation
// unless the query starts with "=".
function evaluateCalculation(query) {
  var raw = String(query === undefined || query === null ? "" : query).trim()
  if (!raw || raw.length > CALC_MAX_LENGTH) return null
  var forced = raw.charAt(0) === "="
  var text = forced ? raw.slice(1).trim() : raw
  if (!text) return null
  if (!forced && !/[0-9π]/.test(text) && !/[a-z]\s*\(/i.test(text)) return null
  // "15% of 80" and "15 % of 80"
  text = text.replace(/(\d+(?:\.\d+)?)\s*%\s*of\s+/gi, "($1/100)*")
  var tokens = calcTokenize(text)
  if (!tokens || tokens.length === 0) return null
  var parsed
  try { parsed = calcParse(tokens) } catch (_error) { return null }
  if (!forced && parsed.operators === 0) return null
  if (typeof parsed.value !== "number" || !isFinite(parsed.value)) return null
  return {
    value: parsed.value,
    display: formatCalcNumber(parsed.value),
    copy: plainCalcNumber(parsed.value),
    expression: raw
  }
}

// --- shared helpers -------------------------------------------------------------

var ANSWER_KINDS = ["calc", "convert", "time", "timer", "ask", "clipboard"]

function isAnswerKind(kind) {
  return ANSWER_KINDS.indexOf(String(kind || "")) >= 0
}

function answerRow(kind, id, title, context, local, extra) {
  var row = {
    id: kind + ":" + id,
    uiUuid: kind + ":" + id,
    kind: kind,
    provider: "Everything",
    title: String(title),
    context: String(context || ""),
    searchTerms: [],
    badges: [],
    iconHints: [],
    active: false,
    local: local || null
  }
  if (extra) for (var key in extra) row[key] = extra[key]
  return row
}

// Precision suited to a converted quantity: 6 significant digits, grouped.
function formatQuantity(value, maxDecimals) {
  if (!isFinite(value)) return ""
  var decimals = maxDecimals === undefined ? 6 : maxDecimals
  var rounded = Number(Number(value).toPrecision(6))
  var factor = Math.pow(10, decimals)
  rounded = Math.round(rounded * factor) / factor
  return formatCalcNumber(rounded)
}

function plainQuantity(value, maxDecimals) {
  var decimals = maxDecimals === undefined ? 6 : maxDecimals
  var rounded = Number(Number(value).toPrecision(6))
  var factor = Math.pow(10, decimals)
  return String(Math.round(rounded * factor) / factor)
}

// "12", "1.5k", "2jt" (juta), "50rb" (ribu), "3m", "1.2bn", or "=12*4".
var AMOUNT_SUFFIXES = {
  k: 1e3, rb: 1e3, ribu: 1e3, thousand: 1e3,
  m: 1e6, mn: 1e6, mio: 1e6, jt: 1e6, juta: 1e6, million: 1e6,
  b: 1e9, bn: 1e9, billion: 1e9, miliar: 1e9
}

function parseAmount(text) {
  var raw = String(text || "").trim().toLowerCase().replace(/,(?=\d{3}\b)/g, "")
  if (!raw) return null
  var match = /^(\d+(?:\.\d+)?|\.\d+)\s*([a-z]+)?$/.exec(raw)
  if (match) {
    var base = parseFloat(match[1])
    if (!match[2]) return base
    var factor = AMOUNT_SUFFIXES[match[2]]
    return factor ? base * factor : null
  }
  var calc = evaluateCalculation("=" + raw)
  return calc ? calc.value : null
}

// Split "<left> to|in|into|as|-> <right>" at the last connector.
function splitTarget(text) {
  var match = /^(.*\S)\s+(?:to|in|into|as|->|→|=)\s+(\S.*)$/i.exec(text)
  return match ? { left: match[1].trim(), right: match[2].trim() } : { left: text.trim(), right: "" }
}

// --- currency -------------------------------------------------------------------

var CURRENCY_ALIASES = {
  "$": "USD", "us$": "USD", "usd": "USD", "dollar": "USD", "dollars": "USD", "buck": "USD", "bucks": "USD",
  "s$": "SGD", "sgd": "SGD", "sing": "SGD",
  "rp": "IDR", "idr": "IDR", "rupiah": "IDR",
  "€": "EUR", "eur": "EUR", "euro": "EUR", "euros": "EUR",
  "£": "GBP", "gbp": "GBP", "quid": "GBP",
  "¥": "JPY", "jpy": "JPY", "yen": "JPY",
  "rm": "MYR", "myr": "MYR", "ringgit": "MYR",
  "฿": "THB", "thb": "THB", "baht": "THB",
  "₩": "KRW", "krw": "KRW", "won": "KRW",
  "₹": "INR", "inr": "INR", "rupee": "INR", "rupees": "INR",
  "₱": "PHP", "php": "PHP", "peso": "PHP", "pesos": "PHP",
  "a$": "AUD", "aud": "AUD", "hk$": "HKD", "hkd": "HKD", "c$": "CAD", "cad": "CAD",
  "nz$": "NZD", "nzd": "NZD", "yuan": "CNY", "rmb": "CNY", "cny": "CNY",
  "chf": "CHF",
  "sek": "SEK", "nok": "NOK", "dkk": "DKK", "pln": "PLN", "czk": "CZK", "huf": "HUF",
  "ron": "RON", "try": "TRY", "lira": "TRY", "zar": "ZAR", "rand": "ZAR", "brl": "BRL",
  "mxn": "MXN", "ils": "ILS", "isk": "ISK"
}

var CURRENCY_FORMAT = {
  USD: { prefix: "$", decimals: 2 }, SGD: { prefix: "S$", decimals: 2 },
  IDR: { prefix: "Rp ", decimals: 0 }, EUR: { prefix: "€", decimals: 2 },
  GBP: { prefix: "£", decimals: 2 }, JPY: { prefix: "¥", decimals: 0 },
  MYR: { prefix: "RM ", decimals: 2 }, THB: { prefix: "฿", decimals: 2 },
  KRW: { prefix: "₩", decimals: 0 }, INR: { prefix: "₹", decimals: 2 },
  PHP: { prefix: "₱", decimals: 2 }, AUD: { prefix: "A$", decimals: 2 },
  HKD: { prefix: "HK$", decimals: 2 }, CAD: { prefix: "C$", decimals: 2 },
  NZD: { prefix: "NZ$", decimals: 2 }, CNY: { prefix: "¥", decimals: 2 },
  HUF: { prefix: "", decimals: 0 }, ISK: { prefix: "", decimals: 0 }
}

function currencyCode(token) {
  var key = String(token || "").trim().toLowerCase()
  return Object.prototype.hasOwnProperty.call(CURRENCY_ALIASES, key) ? CURRENCY_ALIASES[key] : ""
}

function roundTo(value, decimals) {
  var factor = Math.pow(10, decimals)
  return Math.round(value * factor) / factor
}

function formatMoney(value, code) {
  var spec = CURRENCY_FORMAT[code] || { prefix: "", decimals: 2 }
  var decimals = Math.abs(value) < 1 && spec.decimals === 0 ? 2 : spec.decimals
  var fixed = roundTo(value, decimals).toFixed(decimals)
  var parts = fixed.split(".")
  var sign = parts[0].charAt(0) === "-" ? "-" : ""
  var whole = parts[0].replace("-", "").replace(/\B(?=(\d{3})+(?!\d))/g, ",")
  var body = whole + (parts.length > 1 ? "." + parts[1] : "")
  return spec.prefix ? sign + spec.prefix + body : sign + body + " " + code
}

function plainMoney(value, code) {
  var spec = CURRENCY_FORMAT[code] || { decimals: 2 }
  var decimals = Math.abs(value) < 1 && spec.decimals === 0 ? 2 : spec.decimals
  return roundTo(value, decimals).toFixed(decimals)
}

// "100 usd", "usd 100", "$100", "1.5jt idr to sgd", "rp 50rb in usd"
function parseCurrencyQuery(text) {
  var split = splitTarget(text)
  var target = split.right ? currencyCode(split.right) : ""
  if (split.right && !target) return null
  var left = split.left.toLowerCase()
  var symbolFirst = /^(us\$|s\$|a\$|hk\$|c\$|nz\$|\$|€|£|¥|฿|₩|₹|₱|rp|rm)\s*(.+)$/.exec(left)
  var codeFirst = /^([a-z]{3})\s+(.+)$/.exec(left)
  var amountFirst = /^(.+?)\s*(us\$|s\$|a\$|hk\$|c\$|nz\$|\$|€|£|¥|฿|₩|₹|₱|[a-z]{2,8})$/.exec(left)
  var candidates = []
  if (symbolFirst) candidates.push({ cur: symbolFirst[1], amount: symbolFirst[2] })
  if (codeFirst) candidates.push({ cur: codeFirst[1], amount: codeFirst[2] })
  if (amountFirst) candidates.push({ cur: amountFirst[2], amount: amountFirst[1] })
  for (var i = 0; i < candidates.length; i++) {
    var source = currencyCode(candidates[i].cur)
    if (!source) continue
    var amount = parseAmount(candidates[i].amount)
    if (amount === null || !isFinite(amount)) continue
    return { amount: amount, from: source, to: target }
  }
  return null
}

function convertMoney(amount, from, to, rates) {
  if (!rates || !rates.rates) return null
  var table = rates.rates
  var base = rates.base
  function perBase(code) { return code === base ? 1 : Number(table[code]) }
  var fromRate = perBase(from)
  var toRate = perBase(to)
  if (!isFinite(fromRate) || !isFinite(toRate) || fromRate <= 0 || toRate <= 0) return null
  return amount / fromRate * toRate
}

function shortDate(iso) {
  var m = /^(\d{4})-(\d{2})-(\d{2})$/.exec(String(iso || ""))
  if (!m) return ""
  var months = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"]
  return Number(m[3]) + " " + months[Number(m[2]) - 1]
}

function currencyAnswers(text, ctx) {
  var parsed = parseCurrencyQuery(text)
  if (!parsed) return null
  var targets = parsed.to ? [parsed.to] : (ctx.currencies || ["USD", "EUR"]).filter(function(code) {
    return code !== parsed.from
  })
  if (targets.length === 0) return null
  if (!ctx.rates) {
    return { needs: { rates: true }, items: [answerRow("convert", "rates-loading",
      ctx.ratesError ? "Exchange rates unavailable" : "Fetching exchange rates…",
      ctx.ratesError ? String(ctx.ratesError) : "ECB via frankfurter.dev", null)] }
  }
  var items = []
  var source = formatMoney(parsed.amount, parsed.from)
  for (var i = 0; i < targets.length && items.length < 4; i++) {
    var value = convertMoney(parsed.amount, parsed.from, targets[i], ctx.rates)
    if (value === null) continue
    items.push(answerRow("convert", "money:" + targets[i], formatMoney(value, targets[i]),
      source + " · ECB " + shortDate(ctx.rates.date),
      { type: "copy", text: plainMoney(value, targets[i]), label: formatMoney(value, targets[i]) },
      { emphasis: true }))
  }
  return items.length ? { needs: { rates: true }, items: items } : null
}

// --- units ----------------------------------------------------------------------

// factor converts one unit to the category's base unit.
var UNITS = [
  { cat: "length", sym: "mm", f: 0.001, a: ["mm", "millimeter", "millimeters", "millimetre", "millimetres"] },
  { cat: "length", sym: "cm", f: 0.01, a: ["cm", "centimeter", "centimeters", "centimetre", "centimetres"] },
  { cat: "length", sym: "m", f: 1, a: ["m", "meter", "meters", "metre", "metres"] },
  { cat: "length", sym: "km", f: 1000, a: ["km", "kilometer", "kilometers", "kilometre", "kilometres", "kms"] },
  { cat: "length", sym: "in", f: 0.0254, a: ["in", "inch", "inches", "\""] },
  { cat: "length", sym: "ft", f: 0.3048, a: ["ft", "foot", "feet", "'"] },
  { cat: "length", sym: "yd", f: 0.9144, a: ["yd", "yard", "yards"] },
  { cat: "length", sym: "mi", f: 1609.344, a: ["mi", "mile", "miles"] },
  { cat: "length", sym: "nmi", f: 1852, a: ["nmi", "nm", "nautical mile", "nautical miles"] },
  { cat: "mass", sym: "mg", f: 0.001, a: ["mg", "milligram", "milligrams"] },
  { cat: "mass", sym: "g", f: 1, a: ["g", "gram", "grams", "gr"] },
  { cat: "mass", sym: "kg", f: 1000, a: ["kg", "kilo", "kilos", "kilogram", "kilograms", "kgs"] },
  { cat: "mass", sym: "t", f: 1e6, a: ["t", "tonne", "tonnes", "ton", "tons"] },
  { cat: "mass", sym: "oz", f: 28.349523125, a: ["oz", "ounce", "ounces"] },
  { cat: "mass", sym: "lb", f: 453.59237, a: ["lb", "lbs", "pound", "pounds"] },
  { cat: "mass", sym: "st", f: 6350.29318, a: ["st", "stone", "stones"] },
  { cat: "volume", sym: "ml", f: 0.001, a: ["ml", "milliliter", "milliliters", "millilitre", "millilitres"] },
  { cat: "volume", sym: "l", f: 1, a: ["l", "liter", "liters", "litre", "litres", "ltr"] },
  { cat: "volume", sym: "tsp", f: 0.00492892159, a: ["tsp", "teaspoon", "teaspoons"] },
  { cat: "volume", sym: "tbsp", f: 0.0147867648, a: ["tbsp", "tablespoon", "tablespoons"] },
  { cat: "volume", sym: "cup", f: 0.2365882365, a: ["cup", "cups"] },
  { cat: "volume", sym: "fl oz", f: 0.0295735296, a: ["floz", "fl oz", "fluid ounce", "fluid ounces"] },
  { cat: "volume", sym: "pt", f: 0.473176473, a: ["pt", "pint", "pints"] },
  { cat: "volume", sym: "qt", f: 0.946352946, a: ["qt", "quart", "quarts"] },
  { cat: "volume", sym: "gal", f: 3.785411784, a: ["gal", "gallon", "gallons"] },
  { cat: "area", sym: "m²", f: 1, a: ["m2", "m^2", "sqm", "sq m", "square meter", "square meters", "square metre", "square metres"] },
  { cat: "area", sym: "km²", f: 1e6, a: ["km2", "km^2", "sq km", "square kilometer", "square kilometers"] },
  { cat: "area", sym: "ft²", f: 0.09290304, a: ["ft2", "ft^2", "sqft", "sq ft", "square foot", "square feet"] },
  { cat: "area", sym: "mi²", f: 2589988.110336, a: ["mi2", "sq mi", "square mile", "square miles"] },
  { cat: "area", sym: "ha", f: 10000, a: ["ha", "hectare", "hectares"] },
  { cat: "area", sym: "acre", f: 4046.8564224, a: ["acre", "acres", "ac"] },
  { cat: "speed", sym: "km/h", f: 1 / 3.6, a: ["km/h", "kmh", "kph", "kmph"] },
  { cat: "speed", sym: "mph", f: 0.44704, a: ["mph"] },
  { cat: "speed", sym: "m/s", f: 1, a: ["m/s", "mps"] },
  { cat: "speed", sym: "kn", f: 0.514444444, a: ["kn", "knot", "knots", "kt"] },
  { cat: "data", sym: "B", f: 1, a: ["b", "byte", "bytes"] },
  { cat: "data", sym: "KB", f: 1e3, a: ["kb", "kilobyte", "kilobytes"] },
  { cat: "data", sym: "MB", f: 1e6, a: ["mb", "megabyte", "megabytes"] },
  { cat: "data", sym: "GB", f: 1e9, a: ["gb", "gigabyte", "gigabytes"] },
  { cat: "data", sym: "TB", f: 1e12, a: ["tb", "terabyte", "terabytes"] },
  { cat: "data", sym: "PB", f: 1e15, a: ["pb", "petabyte", "petabytes"] },
  { cat: "data", sym: "KiB", f: 1024, a: ["kib"] },
  { cat: "data", sym: "MiB", f: 1048576, a: ["mib"] },
  { cat: "data", sym: "GiB", f: 1073741824, a: ["gib"] },
  { cat: "data", sym: "TiB", f: 1099511627776, a: ["tib"] },
  { cat: "duration", sym: "ms", f: 0.001, a: ["ms", "millisecond", "milliseconds"] },
  { cat: "duration", sym: "s", f: 1, a: ["s", "sec", "secs", "second", "seconds"] },
  { cat: "duration", sym: "min", f: 60, a: ["min", "mins", "minute", "minutes"] },
  { cat: "duration", sym: "h", f: 3600, a: ["h", "hr", "hrs", "hour", "hours"] },
  { cat: "duration", sym: "d", f: 86400, a: ["d", "day", "days"] },
  { cat: "duration", sym: "wk", f: 604800, a: ["wk", "week", "weeks"] },
  { cat: "energy", sym: "kcal", f: 4184, a: ["kcal", "calorie", "calories", "cal"] },
  { cat: "energy", sym: "kJ", f: 1000, a: ["kj", "kilojoule", "kilojoules"] },
  { cat: "temperature", sym: "°C", f: 0, a: ["c", "°c", "celsius", "centigrade", "degc"] },
  { cat: "temperature", sym: "°F", f: 0, a: ["f", "°f", "fahrenheit", "degf"] },
  { cat: "temperature", sym: "K", f: 0, a: ["k", "kelvin"] }
]

var UNIT_DEFAULT_TARGETS = {
  "mm": ["in"], "cm": ["in"], "m": ["ft"], "km": ["mi"], "in": ["cm"], "ft": ["m"],
  "yd": ["m"], "mi": ["km"], "nmi": ["km"], "mg": ["g"], "g": ["oz"], "kg": ["lb"],
  "t": ["lb"], "oz": ["g"], "lb": ["kg"], "st": ["kg"], "ml": ["fl oz"], "l": ["gal"],
  "tsp": ["ml"], "tbsp": ["ml"], "cup": ["ml"], "fl oz": ["ml"], "pt": ["ml"], "qt": ["l"],
  "gal": ["l"], "m²": ["ft²"], "km²": ["mi²"], "ft²": ["m²"],
  "mi²": ["km²"], "ha": ["acre"], "acre": ["ha"], "km/h": ["mph"], "mph": ["km/h"],
  "m/s": ["km/h"], "kn": ["km/h"], "B": ["KB"], "KB": ["KiB"], "MB": ["MiB"], "GB": ["GiB", "MB"],
  "TB": ["TiB", "GB"], "PB": ["TB"], "KiB": ["KB"], "MiB": ["MB"], "GiB": ["GB"], "TiB": ["TB"],
  "ms": ["s"], "s": ["min"], "min": ["h"], "h": ["min"], "d": ["h"], "wk": ["d"],
  "kcal": ["kJ"], "kJ": ["kcal"], "°C": ["°F"], "°F": ["°C"], "K": ["°C"]
}

var UNIT_BY_ALIAS = (function() {
  var map = {}
  for (var i = 0; i < UNITS.length; i++) {
    for (var j = 0; j < UNITS[i].a.length; j++) map[UNITS[i].a[j]] = UNITS[i]
  }
  return map
})()

function unitFor(token) {
  var key = String(token || "").trim().toLowerCase().replace(/^degrees?\s+/, "")
  if (Object.prototype.hasOwnProperty.call(UNIT_BY_ALIAS, key)) return UNIT_BY_ALIAS[key]
  return null
}

function unitBySymbol(sym) {
  for (var i = 0; i < UNITS.length; i++) if (UNITS[i].sym === sym) return UNITS[i]
  return null
}

function toKelvin(value, sym) {
  if (sym === "°C") return value + 273.15
  if (sym === "°F") return (value - 32) * 5 / 9 + 273.15
  return value
}

function fromKelvin(value, sym) {
  if (sym === "°C") return value - 273.15
  if (sym === "°F") return (value - 273.15) * 9 / 5 + 32
  return value
}

function convertUnit(value, from, to) {
  if (from.cat !== to.cat) return null
  if (from.cat === "temperature") return fromKelvin(toKelvin(value, from.sym), to.sym)
  return value * from.f / to.f
}

function parseUnitQuery(text) {
  var split = splitTarget(text)
  var target = split.right ? unitFor(split.right) : null
  if (split.right && !target) return null
  var m = /^(-?(?:\d[\d,]*(?:\.\d+)?|\.\d+))\s*(.+)$/.exec(split.left.toLowerCase())
  if (!m) return null
  var unit = unitFor(m[2])
  if (!unit) return null
  if (target && target.cat !== unit.cat) return null
  var value = parseFloat(m[1].replace(/,/g, ""))
  if (!isFinite(value)) return null
  // A bare "5 m" or "3 s" is too ambiguous to answer without a target.
  if (!target && (unit.sym === "m" || unit.sym === "s" || unit.sym === "B" || unit.sym === "K"
      || unit.sym === "d" || unit.sym === "t" || unit.sym === "g") && m[2].length <= 1) return null
  return { value: value, from: unit, to: target }
}

function unitAnswers(text) {
  var parsed = parseUnitQuery(text)
  if (!parsed) return null
  var targets = parsed.to ? [parsed.to]
    : (UNIT_DEFAULT_TARGETS[parsed.from.sym] || []).map(unitBySymbol).filter(Boolean)
  var items = []
  var sourceLabel = formatQuantity(parsed.value) + " " + parsed.from.sym
  for (var i = 0; i < targets.length; i++) {
    var value = convertUnit(parsed.value, parsed.from, targets[i])
    if (value === null || !isFinite(value)) continue
    var shown = formatQuantity(value) + " " + targets[i].sym
    items.push(answerRow("convert", "unit:" + targets[i].sym, shown, sourceLabel,
      { type: "copy", text: plainQuantity(value), label: shown }, { emphasis: true }))
  }
  return items.length ? { items: items } : null
}

// --- time zones -----------------------------------------------------------------

var ZONE_ALIASES = {
  "singapore": "Asia/Singapore", "sg": "Asia/Singapore", "sgt": "Asia/Singapore", "sgp": "Asia/Singapore",
  "jakarta": "Asia/Jakarta", "jkt": "Asia/Jakarta", "wib": "Asia/Jakarta", "indonesia": "Asia/Jakarta",
  "bandung": "Asia/Jakarta", "surabaya": "Asia/Jakarta", "yogyakarta": "Asia/Jakarta", "jogja": "Asia/Jakarta",
  "bali": "Asia/Makassar", "denpasar": "Asia/Makassar", "makassar": "Asia/Makassar", "wita": "Asia/Makassar",
  "jayapura": "Asia/Jayapura", "wit": "Asia/Jayapura",
  "kuala lumpur": "Asia/Kuala_Lumpur", "kl": "Asia/Kuala_Lumpur", "malaysia": "Asia/Kuala_Lumpur",
  "bangkok": "Asia/Bangkok", "thailand": "Asia/Bangkok", "manila": "Asia/Manila", "philippines": "Asia/Manila",
  "ho chi minh": "Asia/Ho_Chi_Minh", "saigon": "Asia/Ho_Chi_Minh", "hanoi": "Asia/Ho_Chi_Minh", "vietnam": "Asia/Ho_Chi_Minh",
  "hong kong": "Asia/Hong_Kong", "hk": "Asia/Hong_Kong", "hkt": "Asia/Hong_Kong",
  "shanghai": "Asia/Shanghai", "beijing": "Asia/Shanghai", "shenzhen": "Asia/Shanghai", "china": "Asia/Shanghai",
  "taipei": "Asia/Taipei", "taiwan": "Asia/Taipei",
  "tokyo": "Asia/Tokyo", "osaka": "Asia/Tokyo", "japan": "Asia/Tokyo", "jst": "Asia/Tokyo",
  "seoul": "Asia/Seoul", "korea": "Asia/Seoul", "kst": "Asia/Seoul",
  "sydney": "Australia/Sydney", "melbourne": "Australia/Melbourne", "canberra": "Australia/Sydney",
  "aest": "Australia/Sydney", "aedt": "Australia/Sydney", "brisbane": "Australia/Brisbane",
  "perth": "Australia/Perth", "awst": "Australia/Perth", "adelaide": "Australia/Adelaide",
  "auckland": "Pacific/Auckland", "wellington": "Pacific/Auckland", "new zealand": "Pacific/Auckland", "nzt": "Pacific/Auckland",
  "delhi": "Asia/Kolkata", "new delhi": "Asia/Kolkata", "mumbai": "Asia/Kolkata", "bangalore": "Asia/Kolkata",
  "bengaluru": "Asia/Kolkata", "india": "Asia/Kolkata", "ist": "Asia/Kolkata",
  "dubai": "Asia/Dubai", "abu dhabi": "Asia/Dubai", "uae": "Asia/Dubai", "riyadh": "Asia/Riyadh",
  "doha": "Asia/Qatar", "istanbul": "Europe/Istanbul", "moscow": "Europe/Moscow", "msk": "Europe/Moscow",
  "london": "Europe/London", "uk": "Europe/London", "bst": "Europe/London", "edinburgh": "Europe/London",
  "dublin": "Europe/Dublin", "ireland": "Europe/Dublin", "lisbon": "Europe/Lisbon", "portugal": "Europe/Lisbon",
  "paris": "Europe/Paris", "france": "Europe/Paris", "berlin": "Europe/Berlin", "germany": "Europe/Berlin",
  "munich": "Europe/Berlin", "amsterdam": "Europe/Amsterdam", "netherlands": "Europe/Amsterdam",
  "brussels": "Europe/Brussels", "madrid": "Europe/Madrid", "barcelona": "Europe/Madrid", "spain": "Europe/Madrid",
  "rome": "Europe/Rome", "milan": "Europe/Rome", "italy": "Europe/Rome", "zurich": "Europe/Zurich",
  "geneva": "Europe/Zurich", "vienna": "Europe/Vienna", "prague": "Europe/Prague", "warsaw": "Europe/Warsaw",
  "stockholm": "Europe/Stockholm", "oslo": "Europe/Oslo", "copenhagen": "Europe/Copenhagen",
  "helsinki": "Europe/Helsinki", "athens": "Europe/Athens", "cet": "Europe/Paris", "cest": "Europe/Paris",
  "eet": "Europe/Athens", "cairo": "Africa/Cairo", "johannesburg": "Africa/Johannesburg",
  "lagos": "Africa/Lagos", "nairobi": "Africa/Nairobi",
  "new york": "America/New_York", "nyc": "America/New_York", "ny": "America/New_York", "boston": "America/New_York",
  "washington": "America/New_York", "miami": "America/New_York", "toronto": "America/Toronto",
  "est": "America/New_York", "edt": "America/New_York", "et": "America/New_York", "eastern": "America/New_York",
  "chicago": "America/Chicago", "austin": "America/Chicago", "dallas": "America/Chicago",
  "cst": "America/Chicago", "cdt": "America/Chicago", "central": "America/Chicago",
  "denver": "America/Denver", "mst": "America/Denver", "mdt": "America/Denver", "mountain": "America/Denver",
  "phoenix": "America/Phoenix", "los angeles": "America/Los_Angeles", "la": "America/Los_Angeles",
  "san francisco": "America/Los_Angeles", "sf": "America/Los_Angeles", "seattle": "America/Los_Angeles",
  "pst": "America/Los_Angeles", "pdt": "America/Los_Angeles", "pt": "America/Los_Angeles",
  "pacific": "America/Los_Angeles", "vancouver": "America/Vancouver", "mexico city": "America/Mexico_City",
  "sao paulo": "America/Sao_Paulo", "brazil": "America/Sao_Paulo", "buenos aires": "America/Argentina/Buenos_Aires",
  "honolulu": "Pacific/Honolulu", "hawaii": "Pacific/Honolulu", "anchorage": "America/Anchorage",
  "utc": "UTC", "gmt": "UTC", "zulu": "UTC"
}

function zoneList() {
  var seen = {}
  var out = []
  for (var key in ZONE_ALIASES) {
    var zone = ZONE_ALIASES[key]
    if (!seen[zone]) { seen[zone] = true; out.push(zone) }
  }
  return out
}

function zoneFor(token) {
  var key = String(token || "").trim().toLowerCase().replace(/\s+/g, " ").replace(/\s+time$/, "")
  return Object.prototype.hasOwnProperty.call(ZONE_ALIASES, key) ? ZONE_ALIASES[key] : ""
}

function zoneLabel(zone) {
  if (zone === "UTC") return "UTC"
  var leaf = String(zone).split("/").pop().replace(/_/g, " ")
  return leaf
}

// "3pm", "3:30 pm", "15:00", "noon", "midnight" -> minutes after midnight.
function parseClock(text) {
  var t = String(text || "").trim().toLowerCase()
  if (t === "noon" || t === "midday") return 12 * 60
  if (t === "midnight") return 0
  var m = /^(\d{1,2})(?:[:.](\d{2}))?\s*(am|pm|a|p)?$/.exec(t)
  if (!m) return -1
  var h = Number(m[1])
  var min = m[2] ? Number(m[2]) : 0
  if (min > 59) return -1
  if (m[3]) {
    if (h < 1 || h > 12) return -1
    var pm = m[3].charAt(0) === "p"
    h = (h % 12) + (pm ? 12 : 0)
  } else {
    // A bare hour needs a colon to be a time ("15:00"), so "3 tokyo" stays a search.
    if (!m[2] || h > 23) return -1
  }
  return h * 60 + min
}

function pad2(n) { return (n < 10 ? "0" : "") + n }

function clockText(minutes) {
  var m = ((minutes % 1440) + 1440) % 1440
  return pad2(Math.floor(m / 60)) + ":" + pad2(m % 60)
}

function offsetText(deltaMinutes) {
  if (deltaMinutes === 0) return "same time"
  var sign = deltaMinutes > 0 ? "+" : "−"
  var abs = Math.abs(deltaMinutes)
  var h = Math.floor(abs / 60)
  var m = abs % 60
  return sign + (h ? h + "h" : "") + (m ? m + "m" : "")
}

var WEEKDAYS = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"]

function dayShiftText(days) {
  if (days === 0) return ""
  return days > 0 ? " (+" + days + "d)" : " (" + days + "d)"
}

function timeAnswers(text, ctx) {
  var q = String(text || "").trim().toLowerCase()
  var offsets = ctx.tzOffsets || null
  var local = ctx.localZone || "UTC"
  var now = ctx.now || Date.now()

  var mode = ""
  var cityA = ""
  var cityB = ""
  var clock = -1
  var m
  if ((m = /^(?:time|now)\s+(?:in|at)\s+(.+)$/.exec(q)) || (m = /^(.+?)\s+time$/.exec(q))
      || (m = /^now\s+(.+)$/.exec(q))) {
    if (!zoneFor(m[1])) return null
    mode = "now"; cityA = m[1]
  } else if ((m = /^(.+?)\s+(?:in|to)\s+(.+)$/.exec(q)) && parseClock(m[1]) >= 0 && zoneFor(m[2])) {
    mode = "convert"; clock = parseClock(m[1]); cityA = ""; cityB = m[2]
  } else if ((m = /^(\d{1,2}(?:[:.]\d{2})?\s*(?:am|pm|a|p)?|noon|midnight)\s+(.+?)(?:\s+(?:in|to)\s+(.+))?$/.exec(q))
             && parseClock(m[1]) >= 0 && zoneFor(m[2]) && (!m[3] || zoneFor(m[3]))) {
    mode = "convert"; clock = parseClock(m[1]); cityA = m[2]; cityB = m[3] || ""
  } else return null

  if (!offsets) {
    return { needs: { tz: true }, items: [answerRow("time", "loading", "Reading time zones…", "", null)] }
  }
  var fromZone = cityA ? zoneFor(cityA) : local
  var toZone = cityB ? zoneFor(cityB) : local
  var fromOffset = offsets[fromZone]
  var toOffset = offsets[toZone]
  var localOffset = offsets[local]
  if (fromOffset === undefined || toOffset === undefined || localOffset === undefined)
    return { needs: { tz: true }, items: [] }

  var nowUtcMinutes = Math.floor(now / 60000)
  if (mode === "now") {
    var there = nowUtcMinutes + fromOffset
    var here = nowUtcMinutes + localOffset
    var dayThere = Math.floor(there / 1440)
    var dayHere = Math.floor(here / 1440)
    var weekday = WEEKDAYS[((dayThere + 4) % 7 + 7) % 7]
    var title = clockText(there) + " in " + zoneLabel(fromZone)
    return { needs: { tz: true }, items: [answerRow("time", "now:" + fromZone, title,
      weekday + dayShiftText(dayThere - dayHere) + " · " + offsetText(fromOffset - localOffset) + " from you",
      { type: "copy", text: clockText(there), label: title }, { emphasis: true })] }
  }

  // clock is a wall time in fromZone, today there.
  var todayFrom = Math.floor((nowUtcMinutes + fromOffset) / 1440)
  var utc = todayFrom * 1440 + clock - fromOffset
  var target = utc + toOffset
  var dayShift = Math.floor(target / 1440) - todayFrom
  var weekdayTarget = WEEKDAYS[((Math.floor(target / 1440) + 4) % 7 + 7) % 7]
  var resultTitle = clockText(target) + " in " + zoneLabel(toZone) + dayShiftText(dayShift)
  return { needs: { tz: true }, items: [answerRow("time", "at:" + fromZone + ":" + toZone, resultTitle,
    clockText(clock) + " " + zoneLabel(fromZone) + " · " + weekdayTarget,
    { type: "copy", text: clockText(target), label: resultTitle }, { emphasis: true })] }
}

// --- timer ----------------------------------------------------------------------

function parseDuration(text) {
  var t = String(text || "").trim().toLowerCase()
  if (/^\d+(?:\.\d+)?$/.test(t)) return parseFloat(t) * 60
  var re = /(\d+(?:\.\d+)?)\s*(h|hr|hrs|hour|hours|m|min|mins|minute|minutes|s|sec|secs|second|seconds)\b/g
  var total = 0
  var consumed = ""
  var m
  while ((m = re.exec(t)) !== null) {
    var n = parseFloat(m[1])
    var u = m[2].charAt(0)
    total += u === "h" ? n * 3600 : (u === "m" ? n * 60 : n)
    consumed += m[0]
  }
  if (!consumed || t.replace(/\s+/g, "") !== consumed.replace(/\s+/g, "")) return -1
  return total
}

function durationText(minutes) {
  var h = Math.floor(minutes / 60)
  var m = minutes % 60
  if (h && m) return h + " h " + m + " min"
  if (h) return h + (h === 1 ? " hour" : " hours")
  return m + " min"
}

function timerAnswers(text, ctx) {
  var m = /^(?:timer|timers|remind(?:er|ers)?(?:\s+me)?)(?:\s+(.*))?$/i.exec(String(text || "").trim())
  if (!m) return null
  var rest = String(m[1] || "").trim().replace(/^in\s+/i, "")
  var items = []
  if (rest) {
    // Longest leading run that parses as a duration; the remainder is the label.
    var words = rest.split(/\s+/)
    var seconds = -1
    var used = 0
    for (var n = words.length; n >= 1; n--) {
      var s = parseDuration(words.slice(0, n).join(" "))
      if (s > 0) { seconds = s; used = n; break }
    }
    if (seconds > 0) {
      var minutes = Math.max(1, Math.min(10080, Math.ceil(seconds / 60)))
      var label = words.slice(used).join(" ").replace(/^(?:to|for|that)\s+/i, "").trim()
      var at = new Date((ctx.now || Date.now()) + minutes * 60000)
      var argv = ["omarchy", "reminder", String(minutes)]
      if (label) argv.push(label)
      items.push(answerRow("timer", "new", "Remind me in " + durationText(minutes)
        + (label ? " — " + label : ""), "at " + pad2(at.getHours()) + ":" + pad2(at.getMinutes()),
        { type: "exec", argv: argv, notice: label || "Reminder set" }))
    } else {
      // "timer app" and friends are searches, not timers.
      return null
    }
  }
  var active = Array.isArray(ctx.reminders) ? ctx.reminders : []
  for (var i = 0; i < active.length && i < 8; i++) {
    var r = active[i] || {}
    items.push(answerRow("timer", "active:" + String(r.unit || i),
      String(r.label || r.message || "Reminder"),
      "in " + String(r.remaining || "?") + " · " + String(r.atTime || ""), null))
  }
  if (!rest && active.length > 0)
    items.push(answerRow("timer", "clear", "Clear all reminders", active.length + " active",
      { type: "exec", argv: ["omarchy", "reminder", "clear"] }))
  if (!rest && active.length === 0)
    items.push(answerRow("timer", "hint", "No reminders running — try “timer 10m tea”", "", null))
  return { needs: { reminders: true }, items: items, exclusive: true }
}

// --- clipboard history ------------------------------------------------------------

var CLIPBOARD_IMAGE_DIR = "/.local/state/omarchy/clipboard-images/"

function clipPreview(text) {
  var oneLine = String(text || "").replace(/\s+/g, " ").trim()
  return oneLine.length > 160 ? oneLine.slice(0, 160) + "…" : oneLine
}

function clipMeta(text) {
  var s = String(text || "")
  var lines = s.split("\n").length
  if (lines > 1) return lines + " lines"
  if (/^https?:\/\/\S+$/.test(s.trim())) return "link"
  return s.length + (s.length === 1 ? " char" : " chars")
}

function clipboardAnswers(text, ctx) {
  var m = /^(?:clip|clipboard|cb)(?:\s+(.*))?$/i.exec(String(text || "").trim())
  if (!m) return null
  var filter = String(m[1] || "").trim()
  var history = Array.isArray(ctx.clipboard) ? ctx.clipboard : null
  if (!history) return { needs: { clipboard: true }, exclusive: true,
    items: [answerRow("clipboard", "loading", "Reading clipboard history…", "", null)] }
  var home = String(ctx.home || "")
  var terms = Model.normalized(filter).split(" ").filter(Boolean)
  var items = []
  for (var i = 0; i < history.length && items.length < 40; i++) {
    var entry = history[i]
    if (!entry || typeof entry !== "object") continue
    if (entry.type === "text" && typeof entry.text === "string") {
      var hay = Model.normalized(entry.text)
      var ok = true
      for (var t = 0; t < terms.length; t++) if (hay.indexOf(terms[t]) < 0) { ok = false; break }
      if (!ok) continue
      items.push(answerRow("clipboard", "h" + i, clipPreview(entry.text) || "(blank)", clipMeta(entry.text),
        { type: "paste-clip", index: i }, { highlightQuery: filter }))
    } else if (entry.type === "image" && typeof entry.path === "string" && home
               && entry.path.indexOf(home + CLIPBOARD_IMAGE_DIR) === 0
               && entry.path.indexOf("..") < 0 && /^image\/(png|jpeg|gif|webp)$/.test(String(entry.mime || ""))) {
      if (terms.length > 0 && !(terms.length === 1 && "image".indexOf(terms[0]) === 0)) continue
      items.push(answerRow("clipboard", "h" + i, "Image", String(entry.capturedAt || ""),
        { type: "paste-image", path: entry.path, mime: entry.mime },
        { thumbnail: "file://" + entry.path, highlightQuery: "" }))
    }
  }
  if (items.length === 0)
    items.push(answerRow("clipboard", "none", filter ? "Nothing in clipboard history matches" : "Clipboard history is empty", "", null))
  return { needs: { clipboard: true }, items: items, exclusive: true }
}

// --- ask an agent -----------------------------------------------------------------

function askAnswers(text, ctx) {
  var raw = String(text || "")
  var m = /^\s*(?:\?|ask\s)\s*([\s\S]*)$/i.exec(raw)
  if (!m) return null
  var prompt = m[1].trim()
  var where = String(ctx.askLabel || "home")
  var items = []
  if (prompt) {
    items.push(answerRow("ask", "prompt", prompt, "opencode · " + where,
      { type: "ask", prompt: prompt }))
  } else if (ctx.lastAsk && ctx.lastAsk.prompt) {
    items.push(answerRow("ask", "last", "Show last answer: " + ctx.lastAsk.prompt, "opencode · " + where,
      { type: "query", text: "? " + ctx.lastAsk.prompt }))
  } else {
    items.push(answerRow("ask", "hint", "Ask opencode anything — type after the ?", "opencode · " + where, null))
  }
  return { items: items, exclusive: true }
}

// Agent output is untrusted Markdown. Images and raw HTML are neutralised so
// rendering it can never fetch a resource; code keeps its literal characters.
function safeMarkdown(text) {
  var source = String(text === undefined || text === null ? "" : text)
  if (source.length > 40000) source = source.slice(0, 40000) + "\n\n…"
  var parts = source.split(/(```[\s\S]*?```|`[^`\n]*`)/)
  for (var i = 0; i < parts.length; i++) {
    if (i % 2 === 1) continue
    parts[i] = parts[i]
      .replace(/!\[([^\]]*)\]\([^)]*\)/g, "[image: $1]")
      .replace(/!\[([^\]]*)\]\[[^\]]*\]/g, "[image: $1]")
      .replace(/&/g, "&amp;")
      .replace(/</g, "&lt;")
  }
  return parts.join("")
}

// --- entry point --------------------------------------------------------------------

// Instant answers for a query. ctx carries data the service owns (rates,
// zone offsets, clipboard history, reminders, last ask) plus settings.
// Returns { items, exclusive, needs }: exclusive answers replace search.
function instantAnswers(query, ctx) {
  var context = ctx || {}
  var disabled = context.disabledKinds || []
  function allowed(kind) { return disabled.indexOf(kind) < 0 }
  var text = String(query === undefined || query === null ? "" : query)
  var trimmed = text.trim()
  var none = { items: [], exclusive: false, needs: {} }
  if (!trimmed || trimmed.length > 4000) return none

  var prefixed = [
    ["ask", askAnswers], ["clipboard", clipboardAnswers], ["timer", timerAnswers]
  ]
  for (var p = 0; p < prefixed.length; p++) {
    var result = prefixed[p][1](text, context)
    if (result) {
      if (!allowed(prefixed[p][0])) return none
      return { items: result.items || [], exclusive: result.exclusive === true, needs: result.needs || {} }
    }
  }
  if (trimmed.length > CALC_MAX_LENGTH) return none

  var items = []
  var needs = {}
  if (allowed("convert")) {
    var money = currencyAnswers(trimmed, context)
    if (money) { items = items.concat(money.items); if (money.needs) needs.rates = true }
    else {
      var units = unitAnswers(trimmed)
      if (units) items = items.concat(units.items)
    }
  }
  if (items.length === 0 && allowed("time")) {
    var times = timeAnswers(trimmed, context)
    if (times) { items = items.concat(times.items); needs.tz = true }
  }
  if (items.length === 0 && allowed("calc")) {
    var calc = evaluateCalculation(trimmed)
    if (calc) {
      items.push(answerRow("calc", "answer", calc.display, "copy",
        { type: "copy", text: calc.copy, label: calc.display }, { emphasis: true }))
    }
  }
  return { items: items, exclusive: false, needs: needs }
}

// --- service data parsers (pure, bounded) -------------------------------------------

// ECB rates from frankfurter.dev. Anything unexpected is rejected whole.
function validateRates(value) {
  if (!value || typeof value !== "object") return null
  var base = String(value.base || "")
  var date = String(value.date || "")
  if (!/^[A-Z]{3}$/.test(base) || !/^\d{4}-\d{2}-\d{2}$/.test(date)) return null
  if (!value.rates || typeof value.rates !== "object") return null
  var rates = {}
  var count = 0
  for (var code in value.rates) {
    if (!/^[A-Z]{3}$/.test(code)) return null
    var rate = Number(value.rates[code])
    if (!isFinite(rate) || rate <= 0 || rate > 1e9) return null
    rates[code] = rate
    if (++count > 80) return null
  }
  if (count < 5) return null
  var fetchedAt = Number(value.fetchedAt || 0)
  return { base: base, date: date, rates: rates, fetchedAt: isFinite(fetchedAt) ? fetchedAt : 0 }
}

// Lines of "<zone> <+hhmm>" plus "LOCAL <zone>" -> { offsets, localZone }.
function parseZoneOffsets(text) {
  var offsets = {}
  var localZone = ""
  var lines = String(text || "").split("\n")
  for (var i = 0; i < lines.length && i < 400; i++) {
    var line = lines[i].trim()
    var local = /^LOCAL\s+([A-Za-z_]+(?:\/[A-Za-z_+-]+){0,2})$/.exec(line)
    if (local) { localZone = local[1]; continue }
    var m = /^([A-Za-z_]+(?:\/[A-Za-z_+-]+){0,2})\s+([+-])(\d{2})(\d{2})$/.exec(line)
    if (!m) continue
    offsets[m[1]] = (m[2] === "-" ? -1 : 1) * (Number(m[3]) * 60 + Number(m[4]))
  }
  if (localZone && offsets[localZone] === undefined) localZone = ""
  return { offsets: offsets, localZone: localZone || "UTC" }
}

function parseReminders(text) {
  try {
    var value = JSON.parse(String(text || ""))
    if (!value || !Array.isArray(value.reminders)) return []
    return value.reminders.slice(0, 20).map(function(r) {
      return {
        unit: String(r && r.unit || "").slice(0, 120),
        label: String(r && r.label || "").slice(0, 200),
        remaining: String(r && r.remaining || "").slice(0, 40),
        atTime: String(r && r.atTime || "").slice(0, 20)
      }
    })
  } catch (_error) {
    return []
  }
}

function parseClipboardHistory(text) {
  try {
    var value = JSON.parse(String(text || ""))
    if (!Array.isArray(value)) return []
    return value.slice(0, 300).map(function(entry) {
      if (!entry || typeof entry !== "object") return null
      if (entry.type === "text" && typeof entry.text === "string")
        return { type: "text", text: entry.text.length > 20000 ? entry.text.slice(0, 20000) : entry.text }
      if (entry.type === "image" && typeof entry.path === "string")
        return { type: "image", path: entry.path.slice(0, 1000), mime: String(entry.mime || "").slice(0, 40),
          capturedAt: String(entry.capturedAt || "").slice(0, 60) }
      return null
    })
  } catch (_error) {
    return []
  }
}

function emptyAskState() {
  return { status: "", prompt: "", workspace: "", answer: "", parts: [], sessionId: "",
    tool: "", error: "", startedAt: 0, finishedAt: 0, cost: 0 }
}

// A new ask-state object with some fields replaced. QML only notifies a var
// property when it gets a different object, so never mutate in place.
function updatedAskState(state, changes) {
  var next = {}
  for (var key in state) next[key] = state[key]
  for (var change in changes) next[change] = changes[change]
  return next
}

// Fold one `opencode run --format json` line into the ask state.
function applyAskEvent(state, line) {
  var event
  try { event = JSON.parse(String(line || "")) } catch (_error) { return state }
  if (!event || typeof event !== "object") return state
  var next = {}
  for (var key in state) next[key] = state[key]
  var part = event.part && typeof event.part === "object" ? event.part : {}
  if (typeof event.sessionID === "string" && /^ses_[A-Za-z0-9]+$/.test(event.sessionID))
    next.sessionId = event.sessionID
  if (event.type === "text" && typeof part.text === "string") {
    var parts = next.parts.slice()
    var id = String(part.id || parts.length)
    var found = false
    for (var i = 0; i < parts.length; i++) {
      if (parts[i].id === id) { parts[i] = { id: id, text: part.text }; found = true; break }
    }
    if (!found) parts.push({ id: id, text: part.text })
    next.parts = parts
    next.answer = parts.map(function(p) { return p.text }).join("\n\n").slice(0, 60000)
    next.tool = ""
  } else if (event.type === "tool_use") {
    next.tool = String(part.tool || "tool").slice(0, 40)
  } else if (event.type === "step_finish" && part.cost !== undefined) {
    var cost = Number(part.cost)
    if (isFinite(cost)) next.cost = (Number(next.cost) || 0) + cost
  } else if (event.type === "error") {
    var err = event.error && (event.error.message || (event.error.data && event.error.data.message))
    next.error = String(err || "opencode reported an error").slice(0, 400)
  }
  return next
}
