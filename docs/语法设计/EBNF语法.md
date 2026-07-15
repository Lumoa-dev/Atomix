# Atomix EBNF 形式化语法

> 架构版本: v0.1 (设计阶段)
> 最后更新: 2026-07-15
> 用途: 语法参考的单点来源，各文档的语法以本文为准

---

## 约定

| 符号 | 含义 |
|------|------|
| `::=` | 定义为 |
| `\|` | 选择 |
| `[ ... ]` | 可选（0 或 1 次） |
| `{ ... }` | 重复（0 或多次） |
| `( ... )` | 分组 |
| `'...'` | 终结符字面量 |
| `(* ... *)` | 注释 |

---

## 1. 词法 (Lexical)

```
letter          ::= 'a'..'z' | 'A'..'Z' | '_'
digit           ::= '0'..'9'
hex_digit       ::= digit | 'a'..'f' | 'A'..'F'

(* 标识符 *)
identifier      ::= letter { letter | digit }

(* 字面量 *)
int_literal     ::= digit { digit }
                  | '0x' hex_digit { hex_digit }
float_literal   ::= digit { digit } '.' digit { digit }
                  | digit { digit } ('.' digit { digit })? ('e' ['-'] digit { digit })
string_literal  ::= '"' { string_char } '"'
fstring_literal ::= 'f' '"' { fstring_char } '"'
bool_literal    ::= 'true' | 'false'
null_literal    ::= 'null'
bytes_literal   ::= '0x' hex_digit { hex_digit }

(* 字符串内字符 *)
string_char     ::= <任意字符除了 '"' 和 '\'> | escape_seq
fstring_char    ::= <任意字符除了 '"', '\', '{'> | escape_seq | interpolation
escape_seq      ::= '\n' | '\t' | '\r' | '\\' | '\"' | '\0' | '\x' hex_digit hex_digit
interpolation   ::= '{' expression '}'

(* 注释 *)
comment         ::= '#' <行尾>
block_comment   ::= '#!' { <任意字符> } '!#'
```

---

## 2. 文件结构 (File Structure)

```
atomix_file     ::= { use_stmt }
                    [ tools_section ]
                    [ input_section ]
                    [ task_section ]
                    { works_section }
                    [ out_section ]

(* USE 声明 — 不属于任何区 *)
use_stmt        ::= 'USE' identifier [ '.' identifier ] ( '::' 'WORKS' | '::' 'TOOLS' [ '::' identifier { ',' identifier } ] )?
                  | 'FROM' string_literal '>' 'USE' identifier ( '::' 'WORKS' | '::' 'TOOLS' [ '::' identifier { ',' identifier } ] )?
```

---

## 3. 类型 (Types)

```
type_expr       ::= 'int'
                  | 'float'
                  | 'string'
                  | 'bytes'
                  | 'bool'
                  | 'void'
                  | 'null'
                  | 'list' [ '[' type_expr ']' ]
                  | 'dict' [ '[' type_expr ',' type_expr ']' ]
```

---

## 4. 表达式 (Expressions)

```
expression      ::= primary
                  | unary_op expression
                  | expression binary_op expression
                  | expression '?' expression ':' expression (* 未来保留 *)

primary         ::= literal
                  | identifier
                  | 'INPUT' ':' identifier
                  | 'U' '[' identifier ']'
                  | identifier '(' [ expression { ',' expression } ] ')'
                  | '(' expression ')'
                  | list_literal
                  | dict_literal
                  | fstring_literal
                  | anon_fn
                  | 'if' '(' expression ')' expression 'else' expression (* 暂不确定 *)

literal         ::= int_literal | float_literal | string_literal
                  | bool_literal | null_literal | bytes_literal

list_literal    ::= '[' [ expression { ',' expression } ] ']'

dict_literal    ::= '{' [ dict_entry { ',' dict_entry } ] '}'
dict_entry      ::= expression ':' expression

(* 匿名函数 *)
anon_fn         ::= 'fn' '(' [ anon_param { ',' anon_param } ] ')' expression
                  | 'fn' '(' [ anon_param { ',' anon_param } ] ')' block
anon_param      ::= type_expr identifier
                  | identifier

unary_op        ::= '~' | '!' | '-'

binary_op       ::= arithmetic_op | bitwise_op | logical_op | comparison_op
arithmetic_op   ::= '+' | '-' | '*' | '/' | '%'
bitwise_op      ::= '&' | '|' | '^' | '<<' | '>>'
logical_op      ::= '&&' | '||'
comparison_op   ::= '==' | '!=' | '<' | '>' | '<=' | '>=' | 'is'
```

---

## 5. 语句 (Statements)

### 5.1 通用语句（全区可用）

```
stmt            ::= var_decl_stmt
                  | implicit_decl_stmt
                  | annotated_decl_stmt
                  | keyword_decl_stmt
                  | arrow_stmt
                  | assignment_stmt
                  | destructure_stmt
                  | call_func_stmt
                  | callif_stmt
                  | if_stmt
                  | for_stmt
                  | join_stmt
                  | return_stmt
                  | raise_stmt
                  | assert_stmt
                  | for_loop
                  | if_cond
                  | block

(* 变量声明 *)
var_decl_stmt   ::= type_expr identifier [ '=' expression ]

(* 隐式声明 — Python 风格 *)
implicit_decl_stmt ::= identifier '=' expression

(* 注解声明 — Python 注解风格 *)
annotated_decl_stmt ::= identifier ':' type_expr '=' expression

(* 关键字声明 — DSL 风格 *)
keyword_decl_stmt ::= keyword ':' identifier [ ':' type_expr ]

(* 箭头流动 *)
arrow_stmt      ::= expression '=>' identifier [ ':' type_expr ]
                  | identifier [ ':' type_expr ] '<=' expression

(* 赋值 *)
assignment_stmt ::= identifier '=' expression

(* 解构赋值 *)
destructure_stmt ::= list_pattern '=' expression
                   | dict_pattern '=' expression
list_pattern    ::= '[' [ destructure_entry { ',' destructure_entry } [ ',' '*' identifier ] ] ']'
destructure_entry ::= identifier
                   | list_pattern
                   | dict_pattern
dict_pattern    ::= '{' [ dict_destructure_entry { ',' dict_destructure_entry } ] '}'
dict_destructure_entry ::= identifier ':' identifier
                        | identifier

(* 直接函数调用 — 值上下文 *)
call_func_stmt  ::= identifier '(' [ expression { ',' expression } ] ')'

(* 块 *)
block           ::= '{' { stmt } '}'
```

### 5.2 TASK 区语句

```
task_stmt       ::= call_stmt
                  | callif_stmt
                  | if_stmt
                  | for_stmt
                  | join_stmt
                  | wait_stmt
                  | var_decl_stmt
                  | implicit_decl_stmt
                  | assignment_stmt
                  | block
                  | 'OUT' ':' identifier [ ':' type_expr ]

(* CALL *)
call_stmt       ::= 'CALL' ':' identifier '(' [ call_args ] ')' output_mode
call_args       ::= expression { ',' expression }
                  | named_arg { ',' named_arg }
                  | expression { ',' expression } ',' named_arg { ',' named_arg }
named_arg       ::= identifier '=' expression

output_mode     ::= '=>' 'OUT' ':' identifier  (* 移动至产出 *)
                  | '=>' identifier            (* 移动至变量 *)
                  | '=' identifier             (* 复制至变量 *)
                  | '\/'                       (* 管道 *)

(* CALLIF *)
callif_stmt     ::= 'CALLIF' ':' identifier '(' [ call_args ] ')' output_mode
                    'IF' ':' callif_condition [ 'as' identifier ] block
callif_condition ::= ('ISERROR' | 'ISTIMEOUT' | 'ISBIGSIZE')
                    [ 'is' identifier ]
                    [ ('==' | '!=' | '<' | '>' | '<=' | '>=') expression ]

(* IF *)
if_stmt         ::= 'IF' ':' '(' expression ')' block [ 'ELSE' ':' block ]

(* FOR *)
for_stmt        ::= 'FOR' ':' '(' identifier 'in' expression ')' block

(* JOIN *)
join_stmt       ::= 'JOIN' ':' expression { ',' expression } '=>' identifier [ ':' type_expr ]

(* WAIT *)
wait_stmt       ::= 'WAIT' identifier '=' identifier '(' [ call_args ] ')'
                    ('=' | '=>') identifier
                    [ 'IF' ':' '(' callif_condition ')' block ]
```

### 5.3 TOOLS 区语句

```
tools_section   ::= 'TOOLS' '{' { tools_item | pub_stmt } '}'

tools_item      ::= func_def | generic_func_def | exception_def

(* 函数定义 *)
func_def        ::= type_expr ':' 'fn' identifier '(' [ func_params ] ')' block

func_params     ::= func_param { ',' func_param }
func_param      ::= type_expr identifier [ '=' expression ]  (* 默认参数 *)

(* 泛型函数定义 *)
generic_func_def ::= 'GENERIC' '<' generic_param { ',' generic_param } '>'
                     type_expr ':' 'fn' identifier '(' [ func_params ] ')' block
generic_param   ::= identifier [ ':' constraint_expr ]
constraint_expr ::= constraint { '+' constraint }
constraint      ::= 'Comparable' | 'Numeric' | 'Hashable' | 'Iterable' | identifier

(* 异常定义 *)
exception_def   ::= 'EXCEPTION' identifier [ ':' identifier ]

(* 公开声明 *)
pub_stmt        ::= 'PUB' ':' identifier { ',' identifier }

tools_stmt      ::= call_func_stmt | callif_stmt | if_stmt | for_stmt
                  | join_stmt | return_stmt | raise_stmt | assert_stmt
                  | var_decl_stmt | implicit_decl_stmt | assignment_stmt
                  | arrow_stmt | block
```

### 5.4 WORKS 区语句

```
works_section   ::= 'WORKS' ':' identifier [ '(' identifier ')' ] '{'
                        { type_expr identifier }     (* 参数变量 *)
                        { func_def | pub_stmt }      (* 方法 + PUB *)
                        { hook_stmt }                (* 钩子声明 *)
                    '}'

(* 钩子声明: 五元模板 *)
hook_stmt       ::= hook '::' [ condition ] '::' [ code ] '::' [ condition ] '::' hook
                  | hook '::' [ condition ] '::' code '::' hook        (* 四元 *)
                  | hook '::' code '::' [ condition ] '::' hook        (* 四元 *)
                  | hook '::' code '::' hook                           (* 三元 *)
                  | hook '::' code                                     (* 二元上 *)
                  | code '::' hook                                     (* 二元下 *)
                  | hook '::' hook                                     (* 一元 *)

hook            ::= identifier
                  | 'VOID_' digit [ '*' '(' identifier ')' ]

code            ::= identifier [ '(' [ call_args ] ')' ]
                  | '{' { works_stmt } '}'

(* 除 I/O/CALL/WAIT 外，与 TOOLS 一致 *)
works_stmt      ::= call_func_stmt | callif_stmt | if_stmt | for_stmt
                  | join_stmt | return_stmt | raise_stmt | assert_stmt
                  | var_decl_stmt | implicit_decl_stmt | assignment_stmt
                  | arrow_stmt | 'this' '.' identifier
                  | 'this' '.' identifier '(' [ call_args ] ')'
                  | block
```

### 5.5 INPUT 区语句

```
input_section   ::= 'INPUT' ':' '{' { input_stmt } '}'
                  | 'INPUT' ':' input_stmt

input_stmt      ::= io_keyword ':' string_literal
                    [ block ] [ '(' config_params ')' ] [ '[' decorator { ',' decorator } ']' ]
                    arrow identifier [ ':' type_expr ]
                  | identifier [ ':' type_expr ] arrow
                    io_keyword ':' string_literal
                    [ block ] [ '(' config_params ')' ] [ '[' decorator { ',' decorator } ']' ]
                  | 'JOIN' ':' expression { ',' expression } arrow identifier [ ':' type_expr ]

io_keyword      ::= 'HTTP' | 'HTTPS' | 'TCP' | 'UDP' | 'WS' | 'WEBS'
                  | 'FILES' | 'JSON' | 'YAML' | 'TOML' | 'XML' | 'CSV'
                  | 'TXT' | 'BIN' | 'MEM'

config_params   ::= config_param { ',' config_param }
config_param    ::= identifier '=' ( expression | string_literal | int_literal | bool_literal )

decorator       ::= identifier

arrow           ::= '=>' | '<='

(* INPUT 区的嵌套块内也用相同语法 *)
input_block     ::= '{' { input_stmt } '}'
```

### 5.6 OUT 区语句

```
out_section     ::= 'OUT' ':' '{' { out_stmt } '}'
                  | 'OUT' ':' out_stmt

out_stmt        ::= expression arrow
                    io_keyword ':' string_literal
                    [ block ] [ '(' config_params ')' ] [ '[' decorator { ',' decorator } ']' ]
                  | io_keyword ':' string_literal
                    [ block ] [ '(' config_params ')' ] [ '[' decorator { ',' decorator } ']' ]
                    arrow expression
```

---

## 6. 关键字 (Keywords)

```
keyword ::= 'ASSERT'
          | 'BIN'
          | 'CALL' | 'CALLIF'
          | 'CONST'
          | 'CSV'
          | 'ELSE'
          | 'EXCEPTION'
          | 'FILES'
          | 'FOR'
          | 'GENERIC'
          | 'HTTP' | 'HTTPS'
          | 'IF'
          | 'INPUT'
          | 'JOIN'
          | 'JSON'
          | 'MEM'
          | 'OUT'
          | 'PUB'
          | 'RAISE'
          | 'TCP' | 'TOML'
          | 'TOOLS'
          | 'TXT'
          | 'UDP'
          | 'USE'
          | 'WAIT'
          | 'WEBS'
          | 'WORKS'
          | 'WS'
          | 'XML'
          | 'YAML'
```

---

## 7. 运算符优先级（确认）

| 优先级 | 类别 | 运算符 | 结合性 |
|--------|------|--------|--------|
| 1 (最高) | Primary | `()` `U[]` | 左→右 |
| 2 | Unary | `~` `!` `-` | 右→左 |
| 3 | Multiplicative | `*` `/` `%` | 左→右 |
| 4 | Additive | `+` `-` | 左→右 |
| 5 | Shift | `<<` `>>` | 左→右 |
| 6 | Bitwise AND | `&` | 左→右 |
| 7 | Bitwise XOR | `^` | 左→右 |
| 8 | Bitwise OR | `\|` | 左→右 |
| 9 | Logical AND | `&&` | 左→右 |
| 10 | Logical OR | `\|\|` | 左→右 |
| 11 (最低) | Comparison | `==` `!=` `<` `>` `<=` `>=` `is` | 左→右 |

> 注意：比较运算符优先级低于逻辑运算符，因此 `a > 0 && b > 0` 按 `(a > 0) && (b > 0)` 解析，无需括号。但推荐加括号以提高可读性。

---

## 8. 文档索引

| EBNF 节 | 对应文档 |
|---------|----------|
| §1 词法 | `通用语法.md` §1 |
| §2 文件结构 | `编译行为.md` §1 · `通用语法.md` §6, §10 |
| §3 类型 | `类型系统.md` §2 |
| §4 表达式 | `通用语法.md` §3 |
| §5.1 通用语句 | `通用语法.md` §2 · `TOOLS语法.md` §3 · `WORKS语法.md` §4 |
| §5.2 TASK 语句 | `TASK语法.md` §2-7 · `通用语法.md` §5 |
| §5.3 TOOLS 语句 | `TOOLS语法.md` §3-5 |
| §5.4 WORKS 语句 | `WORKS语法.md` §2-8 |
| §5.5-5.6 IO 语句 | `IO语法.md` §2 · `关键字参考.md` |
| §6 关键字 | `关键字参考.md` · `通用语法.md` §9 |
