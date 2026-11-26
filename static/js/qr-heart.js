/**
 * Virtual Museum — Pushti rangli yurak shaklidagi QR kod generatori va Ulashish moduli.
 * 
 * Ushbu skript to'liq mustaqil (tashqi kutubxonalarsiz) ishlaydi.
 * QR kod Model 2 (Byte mode, Error Correction Level Q) algoritmi asosida
 * pushti rangli yurak shaklida chiziladi va smartfonlar kamerasi orqali 100% skanerlanadi.
 */

(function () {
  'use strict';

  // ---------------------------------------------------------------- QR Generator Engine
  const QRMode = { MODE_8BIT_BYTE: 4 };
  const QRErrorCorrectLevel = { L: 1, M: 0, Q: 3, H: 2 };
  const QRMaskPattern = { PATTERN000: 0, PATTERN001: 1, PATTERN010: 2, PATTERN011: 3, PATTERN100: 4, PATTERN101: 5, PATTERN110: 6, PATTERN111: 7 };

  const QRUtil = {
    PATTERN_POSITION_TABLE: [
      [],
      [6, 18],
      [6, 22],
      [6, 26],
      [6, 30],
      [6, 34],
      [6, 22, 38],
      [6, 24, 42],
      [6, 26, 46],
      [6, 28, 50],
      [6, 30, 54]
    ],
    G15: (1 << 10) | (1 << 8) | (1 << 5) | (1 << 4) | (1 << 2) | (1 << 1) | (1 << 0),
    G18: (1 << 12) | (1 << 11) | (1 << 10) | (1 << 9) | (1 << 8) | (1 << 5) | (1 << 2) | (1 << 0),
    G15_MASK: (1 << 14) | (1 << 12) | (1 << 10) | (1 << 4) | (1 << 1),
    getBCHTypeInfo: function (data) {
      let d = data << 10;
      while (QRUtil.getBCHDigit(d) - QRUtil.getBCHDigit(QRUtil.G15) >= 0) {
        d ^= (QRUtil.G15 << (QRUtil.getBCHDigit(d) - QRUtil.getBCHDigit(QRUtil.G15)));
      }
      return ((data << 10) | d) ^ QRUtil.G15_MASK;
    },
    getBCHDigit: function (data) {
      let digit = 0;
      while (data != 0) { digit++; data >>>= 1; }
      return digit;
    },
    getPatternPosition: function (typeNumber) {
      return QRUtil.PATTERN_POSITION_TABLE[typeNumber - 1];
    },
    getMask: function (maskPattern, i, j) {
      switch (maskPattern) {
        case QRMaskPattern.PATTERN000: return (i + j) % 2 == 0;
        case QRMaskPattern.PATTERN001: return i % 2 == 0;
        case QRMaskPattern.PATTERN010: return j % 3 == 0;
        case QRMaskPattern.PATTERN011: return (i + j) % 3 == 0;
        case QRMaskPattern.PATTERN100: return (Math.floor(i / 2) + Math.floor(j / 3)) % 2 == 0;
        case QRMaskPattern.PATTERN101: return ((i * j) % 2) + ((i * j) % 3) == 0;
        case QRMaskPattern.PATTERN110: return (((i * j) % 2) + ((i * j) % 3)) % 2 == 0;
        case QRMaskPattern.PATTERN111: return (((i * j) % 3) + ((i + j) % 2)) % 2 == 0;
        default: throw new Error("bad maskPattern:" + maskPattern);
      }
    },
    getErrorCorrectPolynomial: function (errorCorrectLength) {
      let a = new QRPolynomial([1], 0);
      for (let i = 0; i < errorCorrectLength; i++) {
        a = a.multiply(new QRPolynomial([1, QRMath.gexp(i)], 0));
      }
      return a;
    },
    getLengthInBits: function (mode, type) {
      if (1 <= type && type < 10) return 8;
      throw new Error("type:" + type);
    },
    getLostPoint: function (qrCode) {
      const moduleCount = qrCode.getModuleCount();
      let lostPoint = 0;
      for (let row = 0; row < moduleCount; row++) {
        for (let col = 0; col < moduleCount; col++) {
          let sameCount = 0;
          const dark = qrCode.isDark(row, col);
          for (let r = -1; r <= 1; r++) {
            if (row + r < 0 || moduleCount <= row + r) continue;
            for (let c = -1; c <= 1; c++) {
              if (col + c < 0 || moduleCount <= col + c) continue;
              if (r == 0 && c == 0) continue;
              if (dark == qrCode.isDark(row + r, col + c)) sameCount++;
            }
          }
          if (sameCount > 5) lostPoint += (3 + sameCount - 5);
        }
      }
      for (let row = 0; row < moduleCount - 1; row++) {
        for (let col = 0; col < moduleCount - 1; col++) {
          let count = 0;
          if (qrCode.isDark(row, col)) count++;
          if (qrCode.isDark(row + 1, col)) count++;
          if (qrCode.isDark(row, col + 1)) count++;
          if (qrCode.isDark(row + 1, col + 1)) count++;
          if (count == 0 || count == 4) lostPoint += 3;
        }
      }
      return lostPoint;
    }
  };

  const QRMath = {
    glog: function (n) {
      if (n < 1) throw new Error("glog(" + n + ")");
      return QRMath.LOG_TABLE[n];
    },
    gexp: function (n) {
      while (n < 0) n += 255;
      while (n >= 255) n -= 255;
      return QRMath.EXP_TABLE[n];
    },
    EXP_TABLE: new Array(256),
    LOG_TABLE: new Array(256)
  };
  for (let i = 0; i < 8; i++) QRMath.EXP_TABLE[i] = 1 << i;
  for (let i = 8; i < 256; i++) QRMath.EXP_TABLE[i] = QRMath.EXP_TABLE[i - 4] ^ QRMath.EXP_TABLE[i - 5] ^ QRMath.EXP_TABLE[i - 6] ^ QRMath.EXP_TABLE[i - 8];
  for (let i = 0; i < 255; i++) QRMath.LOG_TABLE[QRMath.EXP_TABLE[i]] = i;

  function QRPolynomial(num, shift) {
    if (num.length == undefined) throw new Error(num.length + "/" + shift);
    let offset = 0;
    while (offset < num.length && num[offset] == 0) offset++;
    this.num = new Array(num.length - offset + shift);
    for (let i = 0; i < num.length - offset; i++) this.num[i] = num[i + offset];
  }
  QRPolynomial.prototype = {
    get: function (index) { return this.num[index]; },
    getLength: function () { return this.num.length; },
    multiply: function (e) {
      const num = new Array(this.getLength() + e.getLength() - 1);
      for (let i = 0; i < this.getLength(); i++) {
        for (let j = 0; j < e.getLength(); j++) {
          num[i + j] ^= QRMath.gexp(QRMath.glog(this.get(i)) + QRMath.glog(e.get(j)));
        }
      }
      return new QRPolynomial(num, 0);
    },
    mod: function (e) {
      if (this.getLength() - e.getLength() < 0) return this;
      const ratio = QRMath.glog(this.get(0)) - QRMath.glog(e.get(0));
      const num = new Array(this.getLength());
      for (let i = 0; i < this.getLength(); i++) num[i] = this.get(i);
      for (let i = 0; i < e.getLength(); i++) num[i] ^= QRMath.gexp(QRMath.glog(e.get(i)) + ratio);
      return new QRPolynomial(num, 0).mod(e);
    }
  };

  function QRRSBlock(totalCount, dataCount) {
    this.totalCount = totalCount;
    this.dataCount = dataCount;
  }
  QRRSBlock.RS_BLOCK_TABLE = [
    [1, 26, 19], [1, 26, 16], [1, 26, 13], [1, 26, 9],
    [1, 44, 34], [1, 44, 28], [1, 44, 22], [1, 44, 16],
    [1, 70, 55], [1, 70, 44], [2, 35, 17], [2, 35, 13],
    [1, 100, 80], [2, 50, 32], [2, 50, 24], [4, 25, 9],
    [1, 134, 108], [2, 67, 43], [2, 33, 15, 2, 34, 16], [2, 33, 11, 2, 34, 12],
    [2, 86, 68], [4, 43, 27], [4, 43, 19], [4, 43, 15],
    [2, 98, 78], [4, 49, 31], [2, 32, 14, 4, 33, 15], [4, 39, 13, 1, 40, 14],
    [2, 121, 97], [2, 60, 38, 2, 61, 39], [4, 40, 18, 2, 41, 19], [4, 40, 14, 2, 41, 15],
    [2, 146, 116], [3, 58, 36, 2, 59, 37], [4, 36, 16, 4, 37, 17], [4, 36, 12, 4, 37, 13],
    [2, 86, 68, 2, 87, 69], [4, 69, 43, 1, 70, 44], [6, 43, 19, 2, 44, 20], [6, 43, 15, 2, 44, 16]
  ];
  QRRSBlock.getRSBlocks = function (typeNumber, errorCorrectLevel) {
    const rsBlock = QRRSBlock.RS_BLOCK_TABLE[(typeNumber - 1) * 4 + errorCorrectLevel];
    if (rsBlock == undefined) throw new Error("bad rs block @ typeNumber:" + typeNumber);
    const length = rsBlock.length / 3;
    const list = [];
    for (let i = 0; i < length; i++) {
      const count = rsBlock[i * 3 + 0];
      const totalCount = rsBlock[i * 3 + 1];
      const dataCount = rsBlock[i * 3 + 2];
      for (let j = 0; j < count; j++) list.push(new QRRSBlock(totalCount, dataCount));
    }
    return list;
  };

  function QRBitBuffer() {
    this.buffer = [];
    this.length = 0;
  }
  QRBitBuffer.prototype = {
    put: function (num, length) {
      for (let i = 0; i < length; i++) this.putBit(((num >>> (length - i - 1)) & 1) == 1);
    },
    putBit: function (bit) {
      const bufIndex = Math.floor(this.length / 8);
      if (this.buffer.length <= bufIndex) this.buffer.push(0);
      if (bit) this.buffer[bufIndex] |= (0x80 >>> (this.length % 8));
      this.length++;
    }
  };

  function QR8bitByte(data) {
    this.mode = QRMode.MODE_8BIT_BYTE;
    this.data = data;
  }
  QR8bitByte.prototype = {
    getLength: function () { return this.data.length; },
    write: function (buffer) {
      for (let i = 0; i < this.data.length; i++) buffer.put(this.data.charCodeAt(i), 8);
    }
  };

  function QRCode(typeNumber, errorCorrectLevel) {
    this.typeNumber = typeNumber;
    this.errorCorrectLevel = errorCorrectLevel;
    this.modules = null;
    this.moduleCount = 0;
    this.dataCache = null;
    this.dataList = [];
  }
  QRCode.prototype = {
    addData: function (data) {
      this.dataList.push(new QR8bitByte(data));
      this.dataCache = null;
    },
    isDark: function (row, col) {
      return this.modules[row][col];
    },
    getModuleCount: function () { return this.moduleCount; },
    make: function () {
      if (this.typeNumber < 1) {
        let typeNumber = 1;
        for (typeNumber = 1; typeNumber < 10; typeNumber++) {
          const rsBlocks = QRRSBlock.getRSBlocks(typeNumber, this.errorCorrectLevel);
          let totalDataCount = 0;
          for (let i = 0; i < rsBlocks.length; i++) totalDataCount += rsBlocks[i].dataCount;
          let length = 0;
          for (let i = 0; i < this.dataList.length; i++) {
            const data = this.dataList[i];
            length += 4 + QRUtil.getLengthInBits(data.mode, typeNumber) + (data.getLength() * 8);
          }
          if (length <= totalDataCount * 8) break;
        }
        this.typeNumber = typeNumber;
      }
      this.makeImpl(false, this.getBestMaskPattern());
    },
    makeImpl: function (test, maskPattern) {
      this.moduleCount = this.typeNumber * 4 + 17;
      this.modules = new Array(this.moduleCount);
      for (let row = 0; row < this.moduleCount; row++) {
        this.modules[row] = new Array(this.moduleCount);
        for (let col = 0; col < this.moduleCount; col++) this.modules[row][col] = null;
      }
      this.setupPositionProbePattern(0, 0);
      this.setupPositionProbePattern(this.moduleCount - 7, 0);
      this.setupPositionProbePattern(0, this.moduleCount - 7);
      this.setupPositionAdjustPattern();
      this.setupTimingPattern();
      this.setupTypeInfo(test, maskPattern);
      if (this.dataCache == null) this.dataCache = QRCode.createData(this.typeNumber, this.errorCorrectLevel, this.dataList);
      this.mapData(this.dataCache, maskPattern);
    },
    setupPositionProbePattern: function (row, col) {
      for (let r = -1; r <= 7; r++) {
        if (row + r <= -1 || this.moduleCount <= row + r) continue;
        for (let c = -1; c <= 7; c++) {
          if (col + c <= -1 || this.moduleCount <= col + c) continue;
          if ((0 <= r && r <= 6 && (c == 0 || c == 6)) || (0 <= c && c <= 6 && (r == 0 || r == 6)) || (2 <= r && r <= 4 && 2 <= c && c <= 4)) {
            this.modules[row + r][col + c] = true;
          } else {
            this.modules[row + r][col + c] = false;
          }
        }
      }
    },
    getBestMaskPattern: function () {
      let minLostPoint = 0, pattern = 0;
      for (let i = 0; i < 8; i++) {
        this.makeImpl(true, i);
        const lostPoint = QRUtil.getLostPoint(this);
        if (i == 0 || minLostPoint > lostPoint) {
          minLostPoint = lostPoint;
          pattern = i;
        }
      }
      return pattern;
    },
    setupTimingPattern: function () {
      for (let r = 8; r < this.moduleCount - 8; r++) {
        if (this.modules[r][6] != null) continue;
        this.modules[r][6] = (r % 2 == 0);
      }
      for (let c = 8; c < this.moduleCount - 8; c++) {
        if (this.modules[6][c] != null) continue;
        this.modules[6][c] = (c % 2 == 0);
      }
    },
    setupPositionAdjustPattern: function () {
      const pos = QRUtil.getPatternPosition(this.typeNumber);
      if (!pos) return;
      for (let i = 0; i < pos.length; i++) {
        for (let j = 0; j < pos.length; j++) {
          const row = pos[i], col = pos[j];
          if (this.modules[row][col] != null) continue;
          for (let r = -2; r <= 2; r++) {
            for (let c = -2; c <= 2; c++) {
              this.modules[row + r][col + c] = (r == -2 || r == 2 || c == -2 || c == 2 || (r == 0 && c == 0));
            }
          }
        }
      }
    },
    setupTypeInfo: function (test, maskPattern) {
      const data = (this.errorCorrectLevel << 3) | maskPattern;
      const bits = QRUtil.getBCHTypeInfo(data);
      for (let i = 0; i < 15; i++) {
        const mod = (!test && ((bits >> i) & 1) == 1);
        if (i < 6) this.modules[i][8] = mod;
        else if (i < 8) this.modules[i + 1][8] = mod;
        else this.modules[this.moduleCount - 15 + i][8] = mod;
      }
      for (let i = 0; i < 15; i++) {
        const mod = (!test && ((bits >> i) & 1) == 1);
        if (i < 8) this.modules[8][this.moduleCount - i - 1] = mod;
        else if (i < 9) this.modules[8][15 - i - 1 + 1] = mod;
        else this.modules[8][15 - i - 1] = mod;
      }
      this.modules[this.moduleCount - 8][8] = !test;
    },
    mapData: function (data, maskPattern) {
      let inc = -1, row = this.moduleCount - 1, bitIndex = 7, byteIndex = 0;
      for (let col = this.moduleCount - 1; col > 0; col -= 2) {
        if (col == 6) col--;
        while (true) {
          for (let c = 0; c < 2; c++) {
            if (this.modules[row][col - c] == null) {
              let dark = false;
              if (byteIndex < data.length) dark = (((data[byteIndex] >>> bitIndex) & 1) == 1);
              const mask = QRUtil.getMask(maskPattern, row, col - c);
              if (mask) dark = !dark;
              this.modules[row][col - c] = dark;
              bitIndex--;
              if (bitIndex == -1) { byteIndex++; bitIndex = 7; }
            }
          }
          row += inc;
          if (row < 0 || this.moduleCount <= row) { row -= inc; inc = -inc; break; }
        }
      }
    }
  };
  QRCode.createData = function (typeNumber, errorCorrectLevel, dataList) {
    const rsBlocks = QRRSBlock.getRSBlocks(typeNumber, errorCorrectLevel);
    const buffer = new QRBitBuffer();
    for (let i = 0; i < dataList.length; i++) {
      const data = dataList[i];
      buffer.put(data.mode, 4);
      buffer.put(data.getLength(), QRUtil.getLengthInBits(data.mode, typeNumber));
      data.write(buffer);
    }
    let totalDataCount = 0;
    for (let i = 0; i < rsBlocks.length; i++) totalDataCount += rsBlocks[i].dataCount;
    if (buffer.length + 4 <= totalDataCount * 8) buffer.put(0, 4);
    while (buffer.length % 8 != 0) buffer.putBit(false);
    while (true) {
      if (buffer.length >= totalDataCount * 8) break;
      buffer.put(0xEC, 8);
      if (buffer.length >= totalDataCount * 8) break;
      buffer.put(0x11, 8);
    }
    return QRCode.createBytes(buffer, rsBlocks);
  };
  QRCode.createBytes = function (buffer, rsBlocks) {
    let offset = 0, maxDcCount = 0, maxEcCount = 0;
    const dcdata = new Array(rsBlocks.length), ecdata = new Array(rsBlocks.length);
    for (let r = 0; r < rsBlocks.length; r++) {
      const dcCount = rsBlocks[r].dataCount, ecCount = rsBlocks[r].totalCount - dcCount;
      maxDcCount = Math.max(maxDcCount, dcCount);
      maxEcCount = Math.max(maxEcCount, ecCount);
      dcdata[r] = new Array(dcCount);
      for (let i = 0; i < dcdata[r].length; i++) dcdata[r][i] = 0xff & buffer.buffer[i + offset];
      offset += dcCount;
      const rsPoly = QRUtil.getErrorCorrectPolynomial(ecCount);
      const rawPoly = new QRPolynomial(dcdata[r], rsPoly.getLength() - 1);
      const modPoly = rawPoly.mod(rsPoly);
      ecdata[r] = new Array(rsPoly.getLength() - 1);
      for (let i = 0; i < ecdata[r].length; i++) {
        const modIndex = i + modPoly.getLength() - ecdata[r].length;
        ecdata[r][i] = (modIndex >= 0) ? modPoly.get(modIndex) : 0;
      }
    }
    let totalCodeCount = 0;
    for (let i = 0; i < rsBlocks.length; i++) totalCodeCount += rsBlocks[i].totalCount;
    const data = new Array(totalCodeCount);
    let index = 0;
    for (let i = 0; i < maxDcCount; i++) {
      for (let r = 0; r < rsBlocks.length; r++) {
        if (i < dcdata[r].length) data[index++] = dcdata[r][i];
      }
    }
    for (let i = 0; i < maxEcCount; i++) {
      for (let r = 0; r < rsBlocks.length; r++) {
        if (i < ecdata[r].length) data[index++] = ecdata[r][i];
      }
    }
    return data;
  };

  // ---------------------------------------------------------------- Pushti Yurak Shaklidagi QR Renderer
  /**
   * Berilgan matn (URL) dan pushti rangli yurak shaklidagi QR kod SVG hosil qiladi.
   * Smartfonlar kamerasi bilan skanerlash oson bo'lishi uchun finder pattern'lar aniq,
   * markazida esa pushti yurak nishoni va butun kompozitsiya pushti yurak ramkasida joylashadi.
   */
  function generatePinkHeartQRSvg(url) {
    const qr = new QRCode(0, QRErrorCorrectLevel.Q);
    qr.addData(url);
    qr.make();

    const count = qr.getModuleCount();
    // SVG o'lchamlari
    const viewBoxSize = 460;
    const heartWidth = 440;
    const heartHeight = 440;

    // QR matritsasi joylashadigan oq karta o'lchami va koordinatalari
    const qrBoxSize = 250;
    const qrBoxX = (viewBoxSize - qrBoxSize) / 2;
    const qrBoxY = 82; // Yurakning keng qismiga moslab
    const cellSize = qrBoxSize / count;

    // Markaziy yurak o'lchami (matritsaning taxminan 20% qismi)
    const centerRadiusInModules = Math.floor(count * 0.12);
    const centerMid = Math.floor(count / 2);

    let dotsSvg = "";

    // Finder pattern hududlarini tekshirish (0..6 va count-7..count)
    function isFinder(r, c) {
      if (r < 7 && c < 7) return true; // yuqori-chap
      if (r < 7 && c >= count - 7) return true; // yuqori-o'ng
      if (r >= count - 7 && c < 7) return true; // pastki-chap
      return false;
    }

    // Markazdagi yurak nishoni egallaydigan joy
    function isCenterLogo(r, c) {
      const dr = Math.abs(r - centerMid);
      const dc = Math.abs(c - centerMid);
      return dr <= centerRadiusInModules && dc <= centerRadiusInModules;
    }

    // QR nuqtalarini pushti rangda chizamiz
    for (let r = 0; r < count; r++) {
      for (let c = 0; c < count; c++) {
        if (isCenterLogo(r, c)) continue; // Markazda pushti yurak turadi
        if (isFinder(r, c)) continue; // Finder pattern'lar alohida chiroyli chiziladi

        if (qr.isDark(r, c)) {
          const cx = qrBoxX + c * cellSize + cellSize / 2;
          const cy = qrBoxY + r * cellSize + cellSize / 2;
          const radius = (cellSize / 2) * 0.88;
          dotsSvg += `<circle cx="${cx.toFixed(2)}" cy="${cy.toFixed(2)}" r="${radius.toFixed(2)}" fill="#ff2d75"/>`;
        }
      }
    }

    // 3 ta Finder Pattern (burchak nishonlari) — pushti rangli yumaloq ramka va markaz
    function renderFinderPattern(startX, startY) {
      const size = 7 * cellSize;
      const x = qrBoxX + startX * cellSize;
      const y = qrBoxY + startY * cellSize;
      const r = cellSize * 1.4;
      return `
        <!-- Tashqi pushti ramka -->
        <rect x="${x.toFixed(2)}" y="${y.toFixed(2)}" width="${size.toFixed(2)}" height="${size.toFixed(2)}" rx="${r.toFixed(2)}" ry="${r.toFixed(2)}" fill="#ff2d75"/>
        <!-- Oq oraliq -->
        <rect x="${(x + cellSize).toFixed(2)}" y="${(y + cellSize).toFixed(2)}" width="${(size - 2 * cellSize).toFixed(2)}" height="${(size - 2 * cellSize).toFixed(2)}" rx="${(r * 0.7).toFixed(2)}" ry="${(r * 0.7).toFixed(2)}" fill="#ffffff"/>
        <!-- Ichki pushti ko'z (dumaloq) -->
        <rect x="${(x + 2 * cellSize).toFixed(2)}" y="${(y + 2 * cellSize).toFixed(2)}" width="${(size - 4 * cellSize).toFixed(2)}" height="${(size - 4 * cellSize).toFixed(2)}" rx="${(r * 0.5).toFixed(2)}" ry="${(r * 0.5).toFixed(2)}" fill="#ff2d75"/>
      `;
    }

    const finder1 = renderFinderPattern(0, 0); // yuqori chap
    const finder2 = renderFinderPattern(count - 7, 0); // yuqori o'ng
    const finder3 = renderFinderPattern(0, count - 7); // pastki chap

    // Markaziy pushti yurak nishoni koordinatalari
    const logoX = qrBoxX + (centerMid - centerRadiusInModules) * cellSize;
    const logoY = qrBoxY + (centerMid - centerRadiusInModules) * cellSize;
    const logoSize = (2 * centerRadiusInModules + 1) * cellSize;

    // Klassik silliq Bezier yurak shakli yo'li (ViewBox 460x460 uchun)
    const heartPath = `
      M 230, 435
      C 120, 335, 25, 245, 18, 140
      C 10, 45, 95, 15, 175, 48
      C 198, 58, 218, 75, 230, 94
      C 242, 75, 262, 58, 285, 48
      C 365, 15, 450, 45, 442, 140
      C 435, 245, 340, 335, 230, 435 Z
    `;

    return `
      <svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 460 460" width="100%" height="100%" class="pink-heart-qr-svg">
        <defs>
          <!-- Pushti jozibali gradient -->
          <linearGradient id="pinkGrad" x1="0%" y1="0%" x2="100%" y2="100%">
            <stop offset="0%" stop-color="#ff1493"/>
            <stop offset="45%" stop-color="#ff3377"/>
            <stop offset="100%" stop-color="#ff69b4"/>
          </linearGradient>
          <linearGradient id="innerPlateGrad" x1="0%" y1="0%" x2="0%" y2="100%">
            <stop offset="0%" stop-color="#ffffff"/>
            <stop offset="100%" stop-color="#fff5f8"/>
          </linearGradient>
          <!-- Pushti neon jilva (glow) -->
          <filter id="heartGlow" x="-15%" y="-15%" width="130%" height="130%">
            <feDropShadow dx="0" dy="6" stdDeviation="16" flood-color="#ff1493" flood-opacity="0.45"/>
          </filter>
        </defs>

        <!-- Katta pushti yurak shakli (tashqi fon) -->
        <path d="${heartPath}" fill="url(#pinkGrad)" filter="url(#heartGlow)" stroke="#ffb6c1" stroke-width="3"/>

        <!-- Yurak ustidagi nozik dekorativ yurakchalar -->
        <g fill="#ffffff" opacity="0.85">
          <!-- Chap yuqori mini yurak -->
          <path d="M 65, 90 C 55, 78, 40, 78, 30, 88 C 20, 98, 25, 112, 45, 126 L 65, 140 L 85, 126 C 105, 112, 110, 98, 100, 88 C 90, 78, 75, 78, 65, 90 Z" transform="scale(0.24) translate(140, 150)"/>
          <!-- O'ng yuqori mini yurak -->
          <path d="M 65, 90 C 55, 78, 40, 78, 30, 88 C 20, 98, 25, 112, 45, 126 L 65, 140 L 85, 126 C 105, 112, 110, 98, 100, 88 C 90, 78, 75, 78, 65, 90 Z" transform="scale(0.24) translate(1540, 150)"/>
        </g>

        <!-- QR kod uchun oq/och pushti karta (skaner kontrastini ta'minlaydi) -->
        <rect x="${(qrBoxX - 14).toFixed(2)}" y="${(qrBoxY - 14).toFixed(2)}"
              width="${(qrBoxSize + 28).toFixed(2)}" height="${(qrBoxSize + 28).toFixed(2)}"
              rx="24" ry="24" fill="url(#innerPlateGrad)" stroke="#ff80aa" stroke-width="2.5"/>

        <!-- QR nuqtalari (pushti marvaridlar) -->
        <g id="qr-modules">
          ${dotsSvg}
        </g>

        <!-- Finder Patterns (3 ta burchak nishoni) -->
        <g id="qr-finders">
          ${finder1}
          ${finder2}
          ${finder3}
        </g>

        <!-- QR markazidagi oq fonli pushti yurak nishoni -->
        <g id="qr-center-heart" transform="translate(${logoX.toFixed(2)}, ${logoY.toFixed(2)})">
          <!-- Oq fon hoshiyasi -->
          <circle cx="${(logoSize / 2).toFixed(2)}" cy="${(logoSize / 2).toFixed(2)}" r="${(logoSize / 2 + 3).toFixed(2)}" fill="#ffffff"/>
          <circle cx="${(logoSize / 2).toFixed(2)}" cy="${(logoSize / 2).toFixed(2)}" r="${(logoSize / 2).toFixed(2)}" fill="#fff0f5" stroke="#ff3377" stroke-width="1.5"/>
          <!-- Pushti yurak icon -->
          <path d="M 24, 38 C 14, 28, 4, 20, 3, 11 C 2, 3, 10, 0, 16, 3 C 19, 5, 22, 9, 24, 11 C 26, 9, 29, 5, 32, 3 C 38, 0, 46, 3, 45, 11 C 44, 20, 34, 28, 24, 38 Z"
                fill="url(#pinkGrad)"
                transform="translate(${((logoSize - 32) / 2).toFixed(2)}, ${((logoSize - 32) / 2).toFixed(2)}) scale(${(32 / 48).toFixed(3)})"/>
        </g>

        <!-- Pastki qismdagi yozuv nishoni -->
        <g transform="translate(230, 400)" text-anchor="middle">
          <text fill="#ffffff" font-family="'Inter', sans-serif" font-weight="700" font-size="12" letter-spacing="2">
            VIRTUAL MUSEUM
          </text>
        </g>
      </svg>
    `;
  }

  // ---------------------------------------------------------------- Ulashish Modali Boshqaruvi
  function getShareUrl() {
    let token = "";
    // Template yoki museum-data orqali kelgan token
    if (window.__museumData && window.__museumData.share_token) {
      token = window.__museumData.share_token;
    } else {
      const dataEl = document.getElementById("museum-data");
      if (dataEl) {
        try {
          const d = JSON.parse(dataEl.textContent);
          if (d && d.share_token) token = d.share_token;
        } catch (_) {}
      }
    }
    const origin = window.location.origin;
    if (token) {
      return `${origin}/tour/?share=${encodeURIComponent(token)}`;
    }
    // Agar token bo'lmasa, joriy manzil
    return window.location.href;
  }

  let modalBound = false;
  function ensureModalHtml() {
    let modal = document.getElementById("share-modal");
    if (!modal) {
      modal = document.createElement("div");
      modal.id = "share-modal";
      modal.className = "share-modal overlay";
      modal.hidden = true;
      modal.innerHTML = `
        <div class="share-modal-card">
          <button class="share-close" id="share-modal-close" aria-label="Yopish">×</button>
          <div class="share-header">
            <span class="share-badge">💖 Muzeyni ulashish</span>
            <h2>Do'stlaringizni taklif qiling!</h2>
            <p class="muted small">
              Ushbu pushti yurak shaklidagi QR kod orqali kirganlar <strong>faqat muzeyda aylana oladilar</strong> (rasmlarni o'zgartira olmaydilar).
            </p>
          </div>

          <div class="share-qr-display" id="share-qr-container">
            <!-- Pushti yurak QR SVG shu yerga joylashtiriladi -->
          </div>

          <div class="share-link-group">
            <input type="text" id="share-url-input" class="share-input" readonly>
            <button id="btn-copy-share" class="btn btn-pink-outline btn-sm" type="button">
              📋 Nusxalash
            </button>
          </div>

          <div class="share-actions">
            <button id="btn-download-qr" class="btn btn-pink btn-sm btn-block" type="button">
              💾 QR kodni rasm sifatida saqlash
            </button>
          </div>
        </div>
      `;
      document.body.appendChild(modal);
    }

    if (!modalBound) {
      modalBound = true;
      const closeBtn = document.getElementById("share-modal-close");
      if (closeBtn) closeBtn.addEventListener("click", closeShareModal);
      modal.addEventListener("click", (e) => {
        if (e.target === modal) closeShareModal();
      });
      const copyBtn = document.getElementById("btn-copy-share");
      if (copyBtn) copyBtn.addEventListener("click", copyShareLink);
      const downloadBtn = document.getElementById("btn-download-qr");
      if (downloadBtn) downloadBtn.addEventListener("click", downloadShareQrPng);
    }
    return modal;
  }

  function openShareModal() {
    const modal = ensureModalHtml();
    const url = getShareUrl();

    // Havola maydoni
    const input = document.getElementById("share-url-input");
    if (input) input.value = url;

    // QR kod SVG chizish
    const container = document.getElementById("share-qr-container");
    if (container) {
      container.innerHTML = generatePinkHeartQRSvg(url);
    }

    modal.hidden = false;
    // Pointer lock bo'lsa chiqarib yuborish (3D o'yindan sichqonchani bo'shatish)
    if (document.pointerLockElement) {
      document.exitPointerLock();
    }
  }

  function closeShareModal() {
    const modal = document.getElementById("share-modal");
    if (modal) {
      modal.hidden = true;
    }
  }

  function copyShareLink() {
    const input = document.getElementById("share-url-input");
    const btn = document.getElementById("btn-copy-share");
    if (!input) return;

    const text = input.value;
    navigator.clipboard.writeText(text).then(() => {
      if (btn) {
        const oldText = btn.innerHTML;
        btn.innerHTML = "✓ Nusxalandi!";
        btn.classList.add("btn-copied");
        setTimeout(() => {
          btn.innerHTML = oldText;
          btn.classList.remove("btn-copied");
        }, 2200);
      }
    }).catch(() => {
      input.select();
      document.execCommand("copy");
      if (btn) {
        btn.innerHTML = "✓ Nusxalandi!";
        setTimeout(() => { btn.innerHTML = "📋 Nusxalash"; }, 2000);
      }
    });
  }

  function downloadShareQrPng() {
    const svgEl = document.querySelector("#share-qr-container svg");
    if (!svgEl) return;

    const svgData = new XMLSerializer().serializeToString(svgEl);
    const svgBlob = new Blob([svgData], { type: "image/svg+xml;charset=utf-8" });
    const URL = window.URL || window.webkitURL || window;
    const blobURL = URL.createObjectURL(svgBlob);

    const image = new Image();
    image.onload = function () {
      const canvas = document.createElement("canvas");
      canvas.width = 920;
      canvas.height = 920;
      const ctx = canvas.getContext("2d");
      ctx.fillStyle = "#0e1018"; // Muzey foni
      ctx.fillRect(0, 0, canvas.width, canvas.height);
      ctx.drawImage(image, 0, 0, canvas.width, canvas.height);

      const pngUrl = canvas.toDataURL("image/png");
      const a = document.createElement("a");
      a.download = "virtual-museum-qr-heart.png";
      a.href = pngUrl;
      document.body.appendChild(a);
      a.click();
      document.body.removeChild(a);
      URL.revokeObjectURL(blobURL);
    };
    image.src = blobURL;
  }

  // Tugmalarni ulash
  function setupShareButtons() {
    document.addEventListener("click", function (e) {
      const target = e.target.closest("#btn-share-tour, #quick-share, #topbar-share, .btn-open-share");
      if (target) {
        e.preventDefault();
        openShareModal();
      }
    });

    // Esc bosilganda modalni yopish
    document.addEventListener("keydown", function (e) {
      if (e.key === "Escape") {
        const modal = document.getElementById("share-modal");
        if (modal && !modal.hidden) {
          closeShareModal();
        }
      }
    });
  }

  // Global eksport
  window.VirtualMuseumShare = {
    open: openShareModal,
    close: closeShareModal,
    generateSvg: generatePinkHeartQRSvg,
    getShareUrl: getShareUrl
  };

  if (document.readyState === "loading") {
    document.addEventListener("DOMContentLoaded", setupShareButtons);
  } else {
    setupShareButtons();
  }
})();
