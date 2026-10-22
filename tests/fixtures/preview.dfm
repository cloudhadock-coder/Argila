object Demo: TDemo
  Left = 0
  Top = 0
  Caption = 'Inspeção VCL'
  ClientWidth = 800
  ClientHeight = 560
  object Header: TPanel
    Height = 48
    Align = alTop
    Caption = 'Argila — prévia somente leitura'
  end
  object Footer: TPanel
    Height = 38
    Align = alBottom
    Caption = 'Nenhum evento é executado'
  end
  object NameLabel: TLabel
    Left = 16
    Top = 62
    Width = 100
    Caption = 'Nome completo'
  end
  object NameEdit: TEdit
    Left = 126
    Top = 58
    Width = 300
    Text = 'João da Silva'
    Anchors = [akLeft, akTop, akRight]
    OnChange = NameEditChange
  end
  object SaveButton: TButton
    Left = 690
    Top = 58
    Width = 90
    Caption = '&Salvar'
    Anchors = [akTop, akRight]
    OnClick = SaveButtonClick
  end
  object Choice: TComboBox
    Left = 16
    Top = 96
    Width = 190
    Text = 'Primeiro'
    Items.Strings = (
      'Primeiro'
      'Segundo')
  end
  object EnabledCheck: TCheckBox
    Left = 224
    Top = 98
    Caption = 'Ativo'
    Checked = True
  end
  object Radio: TRadioButton
    Left = 350
    Top = 98
    Caption = 'Preferencial'
    Checked = True
  end
  object Group: TGroupBox
    Left = 16
    Top = 134
    Width = 240
    Height = 142
    Caption = 'Observações'
    object Notes: TMemo
      Align = alClient
      Lines.Strings = (
        'Primeira linha'
        'Segunda linha com '#231#227'o')
    end
  end
  object List: TListBox
    Left = 272
    Top = 134
    Width = 160
    Height = 142
    Items.Strings = (
      'Um'
      'Dois'
      'Três')
  end
  object DatabaseEdit: TDBEdit
    Left = 448
    Top = 134
    Width = 170
    DataField = 'NOME'
    Enabled = False
  end
  object Vendor: TVendorWidget
    Left = 448
    Top = 174
    Width = 230
    Height = 40
    Vendor.Unknown = 'Preservado'
    Picture.Data = {00FF1234ABCD}
  end
  object HiddenButton: TButton
    Left = 448
    Top = 230
    Visible = False
    Caption = 'Oculto'
  end
  object Pages: TPageControl
    Left = 16
    Top = 294
    Width = 764
    Height = 200
    Anchors = [akLeft, akTop, akRight, akBottom]
    ActivePage = DataPage
    object DataPage: TTabSheet
      Caption = 'Dados'
      object Grid: TDBGrid
        Align = alClient
        Columns = <
          item
            FieldName = 'ID'
            Title.Caption = 'Código'
          end
          item
            FieldName = 'NOME'
            Title.Caption = 'Nome'
          end>
      end
    end
    object OtherPage: TTabSheet
      Caption = 'Outra página'
      object OtherLabel: TLabel
        Left = 12
        Top = 12
        Caption = 'Selecione esta página na árvore'
        Width = 300
      end
    end
  end
end
