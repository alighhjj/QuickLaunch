interface Props {
  text: string;
  /** [起始字符序号, 长度]，由后端按「字符」而非字节计算 */
  ranges: [number, number][];
}

interface Segment {
  text: string;
  hit: boolean;
}

function buildSegments(text: string, ranges: [number, number][]): Segment[] {
  const chars = Array.from(text);
  if (ranges.length === 0) {
    return [{ text, hit: false }];
  }

  const marked = new Array<boolean>(chars.length).fill(false);
  for (const [start, length] of ranges) {
    for (let i = start; i < start + length && i < chars.length; i += 1) {
      if (i >= 0) {
        marked[i] = true;
      }
    }
  }

  const segments: Segment[] = [];
  let buffer = "";
  let bufferHit = marked[0] ?? false;
  for (let i = 0; i < chars.length; i += 1) {
    if (marked[i] === bufferHit) {
      buffer += chars[i];
    } else {
      segments.push({ text: buffer, hit: bufferHit });
      buffer = chars[i];
      bufferHit = marked[i];
    }
  }
  segments.push({ text: buffer, hit: bufferHit });
  return segments;
}

/** 关键词高亮。拼音命中的条目后端不下发区间，此处自然退化为整段普通文本。 */
export function Highlight({ text, ranges }: Props) {
  const segments = buildSegments(text, ranges);
  if (segments.length === 1) {
    return <>{text}</>;
  }
  return (
    <>
      {segments.map((segment, index) =>
        segment.hit ? (
          <em key={index} className="hl">
            {segment.text}
          </em>
        ) : (
          <span key={index}>{segment.text}</span>
        ),
      )}
    </>
  );
}
