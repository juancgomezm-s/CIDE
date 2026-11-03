//! Declarative textual ABL templates. No compiler/version certification or code
//! execution. The generic host owns parameters, preview, encoding and undo.
use jc_app::snippets::{ParameterRule, SnippetParameter, SnippetTemplate};
const NAME: SnippetParameter = SnippetParameter {
    key: "name",
    label: "Nombre",
    default: "newProcedure",
    rule: ParameterRule::Identifier { extra: "-." },
};
const CONDITION: SnippetParameter = SnippetParameter {
    key: "condition",
    label: "Condición",
    default: "TRUE",
    rule: ParameterRule::SingleLine,
};
pub static TEMPLATES:&[SnippetTemplate]=&[
 SnippetTemplate{id:"progress.abl.snippet.procedure",name:"ABL Procedimiento",parameters:&[NAME],body:"PROCEDURE ${name}:\n    ${cursor}\nEND PROCEDURE.\n",surround:false},
 SnippetTemplate{id:"progress.abl.snippet.function",name:"ABL Función",parameters:&[SnippetParameter{default:"newFunction",..NAME},SnippetParameter{key:"return_type",label:"Tipo de retorno",default:"LOGICAL",rule:ParameterRule::Identifier{extra:"-."}}],body:"FUNCTION ${name} RETURNS ${return_type} ():\n    ${cursor}\n    RETURN ?.\nEND FUNCTION.\n",surround:false},
 SnippetTemplate{id:"progress.abl.snippet.trigger",name:"ABL Disparador",parameters:&[SnippetParameter{key:"event",label:"Evento",default:"CHOOSE",rule:ParameterRule::Identifier{extra:"-"}},SnippetParameter{key:"widget",label:"Expresión del control",default:"myButton",rule:ParameterRule::SingleLine}],body:"ON ${event} OF ${widget} DO:\n    ${cursor}\nEND.\n",surround:false},
 SnippetTemplate{id:"progress.abl.surround.if",name:"ABL IF THEN DO",parameters:&[CONDITION],body:"IF ${condition} THEN DO:\n${selection}\nEND.${cursor}",surround:true},
 SnippetTemplate{id:"progress.abl.surround.do",name:"ABL bloque DO",parameters:&[],body:"DO:\n${selection}\nEND.${cursor}",surround:true},
 SnippetTemplate{id:"progress.abl.surround.repeat",name:"ABL bloque REPEAT",parameters:&[],body:"REPEAT:\n${selection}\nEND.${cursor}",surround:true},
 SnippetTemplate{id:"progress.abl.surround.for_each",name:"ABL FOR EACH",parameters:&[SnippetParameter{key:"table",label:"Nombre de tabla/búfer",default:"customer",rule:ParameterRule::Identifier{extra:"-."}}],body:"FOR EACH ${table} NO-LOCK:\n${selection}\nEND.${cursor}",surround:true},
];
