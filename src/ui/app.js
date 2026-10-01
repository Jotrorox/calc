// Text-first calculator UI. The API is stateless, so the browser keeps definitions and `ans`
// and sends them as request context on every evaluation.
(() => {
  'use strict';

  const STORAGE_KEY = 'calc.ui.v1';
  const MAX_HISTORY = 200;
  const NAME_START = /[A-Za-z_Ͱ-Ͽ]/;
  const NAME_TAIL = /[A-Za-z0-9_Ͱ-Ͽ₀-₉]*/;
  const WORD_BEFORE_CURSOR = new RegExp(NAME_START.source + NAME_TAIL.source + '$');
  const NAME_AFTER_CURSOR = new RegExp('^' + NAME_TAIL.source);
  const DEFINITION = new RegExp(
    '^\\s*(' + NAME_START.source + NAME_TAIL.source + ')\\s*(\\([^()]*\\))?\\s*=(?!=)'
  );
  const UNIT_DEFINITION = /^\s*unit\s+([^\s=]+)\s*=/;

  const fn = (name, signature, description) => ({ label: name, signature, description, kind: 'function' });
  const one = (description) => (name) => fn(name, name + '(x)', description(name));
  const trig = (name) => name.replace(/^a(.*)$/, 'inverse $1');

  const FUNCTIONS = [
    ...['sin', 'cos', 'tan', 'csc', 'sec', 'cot', 'asin', 'acos', 'atan', 'acsc', 'asec', 'acot']
      .map(one((n) => trig(n).replace(/\b(sin|cos|tan|csc|sec|cot)\b/, (t) => ({
        sin: 'sine', cos: 'cosine', tan: 'tangent', csc: 'cosecant', sec: 'secant', cot: 'cotangent',
      })[t]))),
    ...['sinh', 'cosh', 'tanh', 'csch', 'sech', 'coth', 'asinh', 'acosh', 'atanh', 'acsch', 'asech', 'acoth']
      .map(one((n) => (n.startsWith('a') ? 'inverse hyperbolic ' + n.slice(1, -1) : 'hyperbolic ' + n.slice(0, -1)))),
    fn('sqrt', 'sqrt(x)', 'square root'),
    fn('cbrt', 'cbrt(x)', 'cube root'),
    fn('root', 'root(x, n)', 'n-th root'),
    fn('exp', 'exp(x)', 'e to the power x'),
    fn('ln', 'ln(x)', 'natural logarithm'),
    fn('log', 'log(x) · log(x, base)', 'logarithm, base 10 by default'),
    fn('abs', 'abs(x)', 'absolute value or magnitude'),
    fn('ceil', 'ceil(x)', 'round up'),
    fn('floor', 'floor(x)', 'round down'),
    fn('round', 'round(x)', 'round to nearest integer'),
    fn('trunc', 'trunc(x)', 'drop the fractional part'),
    fn('frac', 'frac(x)', 'fractional part'),
    fn('sgn', 'sgn(x)', 'sign'),
    fn('Re', 'Re(z)', 'real part'),
    fn('Im', 'Im(z)', 'imaginary part'),
    fn('arg', 'arg(z)', 'complex argument'),
    fn('gamma', 'gamma(x)', 'gamma function'),
    fn('hypot', 'hypot(x, y)', 'sqrt(x² + y²)'),
    fn('gcd', 'gcd(a, b)', 'greatest common divisor'),
    fn('lcm', 'lcm(a, b)', 'least common multiple'),
    fn('nCr', 'nCr(n, r)', 'combinations'),
    fn('comb', 'comb(n, r)', 'combinations'),
    fn('nPr', 'nPr(n, r)', 'permutations count'),
    fn('perm', 'perm(n, r)', 'permutations count'),
    fn('bitcmp', 'bitcmp(x)', 'bitwise complement (32-bit)'),
    fn('bitand', 'bitand(a, b)', 'bitwise and (32-bit)'),
    fn('bitor', 'bitor(a, b)', 'bitwise or (32-bit)'),
    fn('bitxor', 'bitxor(a, b)', 'bitwise xor (32-bit)'),
    fn('bitshift', 'bitshift(x, n)', 'shift left (n > 0) or right'),
    fn('iverson', 'iverson(condition)', '1 if true, 0 if false'),
    fn('average', 'average(a, b, …) · average(v)', 'arithmetic mean'),
    fn('min', 'min(a, b, …) · min(v)', 'smallest value'),
    fn('max', 'max(a, b, …) · max(v)', 'largest value'),
    fn('sum', 'sum(k=1, n, expr) · sum(v)', 'sum over a range or vector'),
    fn('prod', 'prod(k=1, n, expr) · prod(v)', 'product over a range or vector'),
    fn('length', 'length(v)', 'number of elements or rows'),
    fn('sort', 'sort(v)', 'sort a vector'),
    fn('append', 'append(v, x)', 'add an element'),
    fn('perms', 'perms(v)', 'all distinct permutations'),
    fn('permutations', 'permutations(v)', 'all distinct permutations'),
    fn('matrix', 'matrix(((a, b), (c, d)))', 'matrix from rows'),
    fn('diag', 'diag(v)', 'diagonal matrix'),
    fn('transpose', 'transpose(M)', 'matrix transpose'),
    fn('trace', 'trace(M)', 'sum of the diagonal'),
    fn('det', 'det(M)', 'determinant'),
    fn('determinant', 'determinant(M)', 'determinant'),
    fn('integrate', 'integrate(a, b, f(x) dx)', 'definite integral'),
    fn('integral', 'integral(a, b, f(x) dx)', 'definite integral'),
  ];

  const WORDS = [
    { label: 'pi', signature: 'π', description: '3.14159…', kind: 'constant' },
    { label: 'tau', signature: 'τ', description: '2π', kind: 'constant' },
    { label: 'e', signature: 'e', description: '2.71828…', kind: 'constant' },
    { label: 'phi', signature: 'ϕ', description: 'golden ratio', kind: 'constant' },
    { label: 'i', signature: 'i', description: 'imaginary unit', kind: 'constant' },
    { label: 'ans', signature: 'ans', description: 'previous result', kind: 'constant' },
    { label: 'true', signature: '', description: 'boolean', kind: 'keyword' },
    { label: 'false', signature: '', description: 'boolean', kind: 'keyword' },
    { label: 'and', signature: 'a and b', description: 'logical and', kind: 'keyword' },
    { label: 'or', signature: 'a or b', description: 'logical or', kind: 'keyword' },
    { label: 'not', signature: 'not a', description: 'logical not', kind: 'keyword' },
    { label: 'mod', signature: 'a mod b', description: 'remainder', kind: 'keyword' },
    { label: 'to', signature: 'value to unit', description: 'convert units', kind: 'keyword' },
    { label: 'unit', signature: 'unit cm = 100m', description: 'define a unit', kind: 'keyword' },
    { label: 'deg', signature: '30 deg', description: 'degrees', kind: 'unit' },
    { label: 'rad', signature: '(pi/2) rad', description: 'radians', kind: 'unit' },
    { label: 'if', signature: '{ a if cond; b otherwise }', description: 'piecewise', kind: 'keyword' },
    { label: 'otherwise', signature: '{ a if cond; b otherwise }', description: 'piecewise', kind: 'keyword' },
  ];

  const COMMANDS = [
    { name: 'help', usage: ':help', help: 'Show commands, keys, and examples.' },
    { name: 'deg', usage: ':deg', help: 'Use degrees for angles.' },
    { name: 'rad', usage: ':rad', help: 'Use radians for angles (default).' },
    { name: 'precision', usage: ':precision 256', help: 'Binary precision in bits, 32–4096 (default 128).' },
    { name: 'digits', usage: ':digits 20', help: 'Significant digits to display, 1–100 (default 12).' },
    { name: 'vars', usage: ':vars', help: 'List your variables, functions, and units.' },
    { name: 'forget', usage: ':forget name', help: 'Remove one definition.' },
    { name: 'reset', usage: ':reset', help: 'Remove every definition and ans.' },
    { name: 'clear', usage: ':clear', help: 'Clear the history shown on screen (Ctrl+L).' },
    { name: 'copy', usage: ':copy', help: 'Copy the last result.' },
  ];
  const COMMAND_ALIASES = { prec: 'precision', p: 'precision', defs: 'vars', h: 'help', '?': 'help', cls: 'clear' };

  const EXAMPLES = [
    '2 + 3 * 4',
    'sqrt(-4) * (2 + 3i)',
    'sin(30 deg)',
    'f(x) = x^2 + 1',
    'f(3) + ans',
    'x^2 = 64',
    'integrate(0, pi, sin(x) dx)',
    '[1,2;3,4] * [5,6;7,8]',
    'sum(k=1, 100, k)',
    'unit cm = 100m; 250 cm to m',
    'det([2,1;1,3])',
    '{x + y = 10; x - y = 2}',
    '0xff + 0b1010',
    'g(n) = { 1 if n <= 1; n g(n - 1) otherwise }; g(20)',
  ];

  // Split a program into top-level statements; separators inside brackets belong to literals.
  function statements(program) {
    const parts = [];
    let depth = 0;
    let start = 0;
    for (let i = 0; i < program.length; i++) {
      const c = program[i];
      if ('([{⟦'.includes(c)) depth++;
      else if (')]}⟧'.includes(c)) depth = Math.max(0, depth - 1);
      else if (depth === 0 && (c === ';' || c === '\n')) {
        parts.push(program.slice(start, i));
        start = i + 1;
      }
    }
    parts.push(program.slice(start));
    return parts.map((s) => s.trim()).filter(Boolean);
  }

  function definitionOf(statement) {
    const unit = statement.match(UNIT_DEFINITION);
    if (unit) return { key: 'unit ' + unit[1], name: unit[1], kind: 'unit', text: statement };
    const match = statement.match(DEFINITION);
    if (!match) return null;
    const params = match[2] ? match[2].slice(1, -1).trim() : null;
    return {
      key: match[1],
      name: match[1],
      kind: params === null ? 'variable' : 'function',
      signature: params === null ? match[1] : match[1] + '(' + params.replace(/\s*,\s*/g, ', ') + ')',
      text: statement,
    };
  }

  // Round an API decimal string to `digits` significant digits without losing range to floats.
  function roundDecimal(raw, digits) {
    const match = /^(-?)(\d*)\.?(\d*)(?:[eE]([+-]?\d+))?$/.exec(raw);
    if (!match || (!match[2] && !match[3])) return raw;
    const sign = match[1];
    let mantissa = match[2] + match[3];
    let point = match[2].length + Number(match[4] || 0);
    const lead = mantissa.match(/^0*/)[0].length;
    if (lead === mantissa.length) return '0';
    mantissa = mantissa.slice(lead);
    point -= lead;
    // Whole numbers that fit are exact and easier to read in full (20! or 2^64).
    if (mantissa.replace(/0+$/, '').length <= point && point <= 21) digits = Math.max(digits, point);
    if (mantissa.length > digits) {
      const kept = mantissa.slice(0, digits).split('').map(Number);
      if (mantissa.charCodeAt(digits) - 48 >= 5) {
        let i = digits - 1;
        while (i >= 0 && ++kept[i] === 10) kept[i--] = 0;
        if (i < 0) {
          kept.unshift(1);
          kept.pop();
          point++;
        }
      }
      mantissa = kept.join('');
    }
    mantissa = mantissa.replace(/0+$/, '');
    let text;
    // Never pad a rounded value with zeros that look like exact digits.
    if (point > 21 || point < -6 || point > Math.max(digits, mantissa.length)) {
      text = mantissa[0] + (mantissa.length > 1 ? '.' + mantissa.slice(1) : '') + 'E' + (point - 1);
    } else if (point <= 0) {
      text = '0.' + '0'.repeat(-point) + mantissa;
    } else if (point >= mantissa.length) {
      text = mantissa + '0'.repeat(point - mantissa.length);
    } else {
      text = mantissa.slice(0, point) + '.' + mantissa.slice(point);
    }
    return sign + text;
  }

  const isZero = (s) => s === '0' || s === '-0';

  function formatValue(value, digits) {
    switch (value.type) {
      case 'number': {
        const re = roundDecimal(value.real, digits);
        const im = roundDecimal(value.imaginary, digits);
        let text;
        if (isZero(im)) text = re;
        else if (isZero(re)) text = (im === '1' ? '' : im === '-1' ? '-' : im) + 'i';
        else if (im.startsWith('-')) text = re + ' − ' + (im === '-1' ? '' : im.slice(1)) + 'i';
        else text = re + ' + ' + (im === '1' ? '' : im) + 'i';
        return value.unit ? text + ' ' + value.unit : text;
      }
      case 'boolean':
        return String(value.value);
      case 'vector':
        return '(' + value.values.map((v) => formatValue(v, digits)).join(', ') + ')';
      case 'matrix':
        return '[' + value.rows.map((row) => row.map((v) => formatValue(v, digits)).join(', ')).join('; ') + ']';
      default:
        return '';
    }
  }

  // Rebuild a structured result as an expression so `ans` survives without recomputing history.
  function literal(value) {
    switch (value.type) {
      case 'number': {
        const part = (s) => {
          if (!/^-?(\d+\.?\d*|\.\d+)([eE][+-]?\d+)?$/.test(s)) throw new Error('nonfinite');
          const n = s.replace('e', 'E');
          return n.startsWith('-') ? '(' + n + ')' : n;
        };
        const number = isZero(value.imaginary)
          ? part(value.real)
          : '(' + part(value.real) + ' + ' + part(value.imaginary) + '*i)';
        return value.unit ? '(' + number + ' ' + value.unit + ')' : number;
      }
      case 'boolean':
        return String(value.value);
      case 'vector':
        return value.values.length === 1
          ? '[ ' + literal(value.values[0]) + ' ]'
          : '(' + value.values.map(literal).join(', ') + ')';
      case 'matrix':
        return '[ ' + value.rows.map((row) => row.map(literal).join(', ')).join('; ') + ' ]';
      default:
        throw new Error('unknown value');
    }
  }

  function load() {
    try {
      return JSON.parse(localStorage.getItem(STORAGE_KEY)) || {};
    } catch {
      return {};
    }
  }

  window.calculator = () => ({
    history: [],
    inputs: [],
    defs: [],
    ans: null,
    settings: { angle: 'rad', precision: 128, digits: 12 },
    text: '',
    busy: false,
    preview: '',
    previewError: false,
    signature: '',
    suggestions: [],
    active: 0,
    navigated: false,
    copiedId: null,
    recall: -1,
    draft: '',
    nextId: 1,
    commands: COMMANDS,
    examples: EXAMPLES,
    previewTimer: null,
    previewController: null,

    init() {
      const saved = load();
      this.history = Array.isArray(saved.history) ? saved.history.map((e) => ({ ...e, full: false })) : [];
      this.inputs = Array.isArray(saved.inputs) ? saved.inputs : [];
      this.defs = Array.isArray(saved.defs) ? saved.defs : [];
      this.ans = typeof saved.ans === 'string' ? saved.ans : null;
      Object.assign(this.settings, saved.settings || {});
      this.nextId = this.history.reduce((max, e) => Math.max(max, e.id), 0) + 1;
      this.$nextTick(() => {
        this.scrollToEnd();
        this.$refs.input.focus();
      });
    },

    save() {
      try {
        localStorage.setItem(STORAGE_KEY, JSON.stringify({
          history: this.history.slice(-MAX_HISTORY).map(({ full, ...entry }) => entry),
          inputs: this.inputs.slice(-MAX_HISTORY),
          defs: this.defs,
          ans: this.ans,
          settings: this.settings,
        }));
      } catch {
        // Storage can be unavailable (private mode, quota); the session still works.
      }
    },

    // ---- evaluation ----

    context() {
      const context = [];
      if (this.defs.length) context.push(this.defs.map((d) => d.text).join('\n'));
      if (this.ans) context.push(this.ans);
      return context;
    },

    async request(expression, signal) {
      const response = await fetch('/calc', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({
          expression,
          context: this.context(),
          precision: this.settings.precision,
          angle_unit: this.settings.angle,
        }),
        signal,
      });
      let body = null;
      try {
        body = await response.json();
      } catch {
        // Non-JSON bodies are reported below by status.
      }
      if (!response.ok || !body) {
        const error = new Error(body?.error?.message || 'The server answered ' + response.status + '.');
        error.status = response.status;
        throw error;
      }
      return body;
    },

    async submit() {
      const input = this.text.trim();
      if (!input || this.busy) return;
      this.remember(input);
      this.text = '';
      this.clearHints();
      this.resize();
      if (input.startsWith(':')) {
        this.command(input);
        return;
      }
      this.busy = true;
      this.push({ id: this.nextId++, input, kind: 'pending', full: false });
      // Mutate through the reactive proxy so the entry re-renders when the answer arrives.
      const entry = this.history[this.history.length - 1];
      try {
        let body;
        try {
          body = await this.request(input);
        } catch (error) {
          if (error.status !== 503) throw error;
          await new Promise((resolve) => setTimeout(resolve, 1000));
          body = await this.request(input);
        }
        this.learn(input);
        if (body.result) {
          entry.kind = 'result';
          entry.value = body.result.value;
          entry.formatted = body.result.formatted;
          try {
            this.ans = literal(body.result.value);
          } catch {
            this.ans = null;
          }
        } else {
          entry.kind = 'defined';
          const names = statements(input).map(definitionOf).filter(Boolean).map((d) => d.signature || d.key);
          entry.message = names.length ? 'Defined ' + names.join(', ') : 'Done';
        }
      } catch (error) {
        entry.kind = 'error';
        entry.message = error.name === 'TypeError' ? 'Could not reach the calculator. Check your connection.' : error.message;
        if (!this.ans && /\bans\b/.test(input) && /'ans'/.test(entry.message)) {
          entry.message = 'There is no previous result to use as ans yet.';
        }
      }
      this.busy = false;
      this.save();
      this.scrollToEnd();
      this.$nextTick(() => this.$refs.input.focus());
    },

    // Keep definitions from a successful program; later definitions replace earlier ones.
    learn(program) {
      for (const definition of statements(program).map(definitionOf).filter(Boolean)) {
        this.defs = this.defs.filter((d) => d.key !== definition.key);
        this.defs.push(definition);
      }
    },

    push(entry) {
      this.history.push(entry);
      if (this.history.length > MAX_HISTORY) this.history.splice(0, this.history.length - MAX_HISTORY);
      this.scrollToEnd();
    },

    info(input, message, kind = 'info') {
      this.push({ id: this.nextId++, input, kind, message, full: false });
      this.save();
    },

    run(input) {
      this.text = input;
      this.submit();
    },

    prefill(input) {
      this.text = input;
      this.$nextTick(() => {
        const field = this.$refs.input;
        this.resize();
        field.focus();
        field.setSelectionRange(input.length, input.length);
        this.changed();
      });
    },

    command(input) {
      const [rawName, ...rest] = input.slice(1).trim().split(/\s+/);
      const name = COMMAND_ALIASES[rawName] || rawName;
      const arg = rest.join(' ');
      switch (name) {
        case 'help':
          return this.info(input, '', 'help');
        case 'deg':
        case 'rad':
          this.settings.angle = name;
          return this.info(input, name === 'deg' ? 'Angles are now in degrees.' : 'Angles are now in radians.');
        case 'precision': {
          const bits = Number(arg);
          if (!arg) return this.info(input, 'Precision is ' + this.settings.precision + ' bits (≈' + Math.floor(this.settings.precision * Math.log10(2)) + ' decimal digits). Set it with :precision 256.');
          if (!Number.isInteger(bits) || bits < 32 || bits > 4096) return this.info(input, 'Precision must be a whole number of bits from 32 to 4096.', 'error');
          this.settings.precision = bits;
          return this.info(input, 'Precision set to ' + bits + ' bits (≈' + Math.floor(bits * Math.log10(2)) + ' decimal digits).');
        }
        case 'digits': {
          const digits = Number(arg);
          if (!arg) return this.info(input, 'Showing ' + this.settings.digits + ' significant digits. Set it with :digits 20.');
          if (!Number.isInteger(digits) || digits < 1 || digits > 100) return this.info(input, 'Digits must be a whole number from 1 to 100.', 'error');
          this.settings.digits = digits;
          return this.info(input, 'Showing ' + digits + ' significant digits.');
        }
        case 'vars':
          return this.info(input, '', 'vars');
        case 'forget': {
          if (!arg) return this.info(input, 'Name what to forget, for example :forget f.', 'error');
          const before = this.defs.length;
          this.defs = this.defs.filter((d) => d.name !== arg && d.key !== arg);
          return before === this.defs.length
            ? this.info(input, 'Nothing named ' + arg + ' is defined.', 'error')
            : this.info(input, 'Forgot ' + arg + '.');
        }
        case 'reset':
          this.defs = [];
          this.ans = null;
          return this.info(input, 'All definitions and ans were removed.');
        case 'clear':
          this.history = [];
          return this.save();
        case 'copy': {
          const last = [...this.history].reverse().find((e) => e.kind === 'result');
          if (!last) return this.info(input, 'There is no result to copy yet.', 'error');
          this.copy(last);
          return this.info(input, 'Copied ' + last.formatted);
        }
        default:
          return this.info(input, 'Unknown command :' + rawName + '. Type :help for the list.', 'error');
      }
    },

    display(value) {
      return value ? formatValue(value, this.settings.digits) : '';
    },

    isRounded(value) {
      return !!value && formatValue(value, this.settings.digits) !== formatValue(value, Infinity);
    },

    async copy(entry) {
      try {
        await navigator.clipboard.writeText(entry.formatted);
        this.copiedId = entry.id;
        setTimeout(() => {
          if (this.copiedId === entry.id) this.copiedId = null;
        }, 1200);
      } catch {
        // Clipboard access can be denied; the value stays selectable.
      }
    },

    // ---- live preview ----

    schedulePreview() {
      clearTimeout(this.previewTimer);
      this.previewController?.abort();
      const input = this.text.trim();
      const isDefinition = statements(input).some((s) => definitionOf(s));
      if (!input || input.startsWith(':') || isDefinition) {
        this.preview = '';
        return;
      }
      this.previewTimer = setTimeout(async () => {
        const controller = new AbortController();
        this.previewController = controller;
        try {
          const body = await this.request(input, controller.signal);
          if (controller.signal.aborted || this.text.trim() !== input) return;
          this.previewError = false;
          this.preview = body.result ? '= ' + this.display(body.result.value) : '';
        } catch (error) {
          if (controller.signal.aborted || this.text.trim() !== input) return;
          // Busy or unreachable servers are not the user's mistake; stay quiet.
          if (error.name === 'TypeError' || error.status === 503) return;
          this.previewError = true;
          this.preview = error.message;
        }
      }, 350);
    },

    // ---- completion ----

    changed() {
      this.recall = -1;
      this.navigated = false;
      this.resize();
      this.refresh();
      this.schedulePreview();
    },

    cursorMoved(event) {
      if (['ArrowLeft', 'ArrowRight', 'Home', 'End'].includes(event.key)) this.refresh();
    },

    refresh() {
      const field = this.$refs.input;
      const before = this.text.slice(0, field.selectionStart);
      this.signature = this.callSignature(before);
      if (field.selectionStart !== field.selectionEnd) {
        this.suggestions = [];
        return;
      }
      const previous = this.suggestions.map((s) => s.label).join();
      this.suggestions = this.complete(before);
      if (this.suggestions.map((s) => s.label).join() !== previous) {
        this.active = 0;
        this.navigated = false;
      }
    },

    clearHints() {
      this.suggestions = [];
      this.signature = '';
      this.preview = '';
      clearTimeout(this.previewTimer);
      this.previewController?.abort();
    },

    vocabulary() {
      const user = this.defs.map((d) => ({
        label: d.name,
        signature: d.kind === 'unit' ? d.text : d.signature,
        description: 'your ' + d.kind,
        kind: d.kind === 'function' ? 'function' : d.kind === 'unit' ? 'unit' : 'variable',
        user: true,
      }));
      const seen = new Set(user.map((u) => u.label));
      return [...user, ...WORDS, ...FUNCTIONS].filter((item) => {
        if (item.user) return true;
        if (seen.has(item.label)) return false;
        seen.add(item.label);
        return true;
      });
    },

    complete(before) {
      const command = /^\s*:(\S*)$/.exec(before);
      if (command) {
        const prefix = command[1].toLowerCase();
        return COMMANDS.filter((c) => c.name.startsWith(prefix) && c.name !== prefix)
          .map((c) => ({ label: ':' + c.name, signature: c.usage, description: c.help, kind: 'command', replace: before.length - before.indexOf(':') }));
      }
      if (/^\s*:/.test(before)) {
        const forget = /^\s*:forget\s+(\S*)$/.exec(before);
        if (!forget) return [];
        return this.defs.filter((d) => d.name.startsWith(forget[1]) && d.name !== forget[1])
          .map((d) => ({ label: d.name, signature: d.signature || d.text, description: 'your ' + d.kind, kind: 'variable', replace: forget[1].length }));
      }
      const word = WORD_BEFORE_CURSOR.exec(before)?.[0];
      if (!word) return [];
      const preceding = before.slice(0, before.length - word.length);
      // Radix prefixes and exponents (0xff, 1E3) look like names but are number literals.
      if (/^[xob]/.test(word) && /(^|[^\w.])0$/.test(preceding)) return [];
      if (/^E\d*$/.test(word) && /\d\.?$/.test(preceding)) return [];
      const lower = word.toLowerCase();
      const rank = (item) => {
        if (item.label.startsWith(word)) return 0;
        if (item.label.toLowerCase().startsWith(lower)) return 1;
        if (word.length > 1 && item.label.toLowerCase().includes(lower)) return 2;
        return -1;
      };
      const matches = this.vocabulary()
        .map((item) => ({ ...item, rank: rank(item), replace: word.length }))
        .filter((item) => item.rank >= 0 && (item.label !== word || item.kind === 'function'))
        .sort((a, b) => a.rank - b.rank || (b.user ? 1 : 0) - (a.user ? 1 : 0) || a.label.length - b.label.length || a.label.localeCompare(b.label));
      return matches.slice(0, 8);
    },

    accept(index = this.active) {
      const choice = this.suggestions[index];
      if (!choice) return;
      const field = this.$refs.input;
      const cursor = field.selectionStart;
      const start = cursor - choice.replace;
      const tailName = choice.kind === 'command' ? '' : (NAME_AFTER_CURSOR.exec(this.text.slice(cursor))?.[0] || '');
      const after = this.text.slice(cursor + tailName.length);
      let insert = choice.label;
      let caret = insert.length;
      if (choice.kind === 'function' && !after.startsWith('(')) {
        insert += '()';
        caret = insert.length - 1;
      } else if (choice.kind === 'function') {
        caret = insert.length + 1;
      } else if (choice.kind === 'command' && COMMANDS.find((c) => ':' + c.name === choice.label)?.usage.includes(' ')) {
        insert += ' ';
        caret = insert.length;
      }
      this.text = this.text.slice(0, start) + insert + after;
      // Update the field synchronously so a keystroke right after Tab lands at the new caret.
      field.value = this.text;
      field.focus();
      field.setSelectionRange(start + caret, start + caret);
      this.suggestions = [];
      this.resize();
      this.signature = this.callSignature(this.text.slice(0, start + caret));
      this.schedulePreview();
    },

    // Show the signature of the innermost function call around the cursor.
    callSignature(before) {
      let depth = 0;
      for (let i = before.length - 1; i >= 0; i--) {
        const c = before[i];
        if (c === ')') depth++;
        else if (c === '(') {
          if (depth === 0) {
            const name = WORD_BEFORE_CURSOR.exec(before.slice(0, i))?.[0];
            if (!name) continue;
            const item = this.vocabulary().find((v) => v.label === name && v.kind === 'function');
            return item ? item.signature + (item.description ? ' — ' + item.description : '') : '';
          }
          depth--;
        }
      }
      return '';
    },

    // ---- keyboard ----

    key(event) {
      const open = this.suggestions.length > 0;
      const field = this.$refs.input;
      switch (event.key) {
        case 'Enter':
          if (event.shiftKey) return;
          event.preventDefault();
          if (open && this.navigated) this.accept();
          else this.submit();
          return;
        case 'Tab':
          if (!open) return;
          event.preventDefault();
          this.accept();
          return;
        case 'Escape':
          event.preventDefault();
          if (open) this.suggestions = [];
          else {
            this.text = '';
            this.clearHints();
            this.resize();
          }
          return;
        case 'ArrowDown':
        case 'ArrowUp': {
          const down = event.key === 'ArrowDown';
          if (open) {
            event.preventDefault();
            this.navigated = true;
            this.active = (this.active + (down ? 1 : -1) + this.suggestions.length) % this.suggestions.length;
            return;
          }
          const value = this.text;
          const onEdge = down
            ? !value.slice(field.selectionEnd).includes('\n')
            : !value.slice(0, field.selectionStart).includes('\n');
          if (!onEdge || event.shiftKey) return;
          if (this.historyStep(down ? 1 : -1)) event.preventDefault();
          return;
        }
        default:
      }
    },

    globalKey(event) {
      if ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === 'l') {
        event.preventDefault();
        this.command(':clear');
        return;
      }
      // Typing anywhere goes to the input.
      const field = this.$refs.input;
      if (document.activeElement !== field && event.key.length === 1 && !event.ctrlKey && !event.metaKey && !event.altKey
        && !window.getSelection()?.toString()) {
        field.focus();
      }
    },

    remember(input) {
      if (this.inputs[this.inputs.length - 1] !== input) this.inputs.push(input);
      if (this.inputs.length > MAX_HISTORY) this.inputs.shift();
      this.recall = -1;
    },

    historyStep(direction) {
      if (!this.inputs.length) return false;
      if (this.recall === -1) {
        if (direction > 0) return false;
        this.draft = this.text;
        this.recall = this.inputs.length;
      }
      const next = this.recall + direction;
      if (next < 0) return true;
      this.recall = next;
      this.text = next >= this.inputs.length ? this.draft : this.inputs[next];
      if (next >= this.inputs.length) this.recall = -1;
      this.suggestions = [];
      this.$nextTick(() => {
        const field = this.$refs.input;
        field.setSelectionRange(this.text.length, this.text.length);
        this.resize();
        this.schedulePreview();
      });
      return true;
    },

    // ---- layout ----

    resize() {
      const field = this.$refs.input;
      if (!field) return;
      field.style.height = 'auto';
      field.style.height = Math.min(field.scrollHeight, 200) + 'px';
    },

    scrollToEnd() {
      this.$nextTick(() => {
        const log = this.$refs.log;
        if (log) log.scrollTop = log.scrollHeight;
      });
    },
  });

  // Exposed for tests and the browser console.
  window.calculatorInternals = { statements, definitionOf, roundDecimal, formatValue, literal };

  document.addEventListener('alpine:init', () => {
    window.Alpine.data('calculator', window.calculator);
  });
})();
